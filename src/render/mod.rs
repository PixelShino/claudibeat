//! One renderer for every surface: Direct2D and DirectWrite drawing into a
//! premultiplied BGRA WIC bitmap. The same pixels go to a PNG (`--render`)
//! or to a layered window on the taskbar.

pub mod flyout;
pub mod icons;
pub mod strip;
pub mod toast;

use std::cell::Cell;
use std::path::Path;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::core::{HSTRING, Result, w};

/// `0xAARRGGBB`, straight alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    fn d2d(self, opacity: f32) -> D2D1_COLOR_F {
        let c = |shift: u32| ((self.0 >> shift) & 0xFF) as f32 / 255.0;
        D2D1_COLOR_F { r: c(16), g: c(8), b: c(0), a: c(24) * opacity }
    }
}

// The palette of the design: shades of grey only.
pub const TRANSPARENT: Color = Color(0x0000_0000);
pub const INK: Color = Color(0xFF1B_1B1B);
pub const MUTED: Color = Color(0xFF5D_5D5D);
pub const TIME: Color = Color(0xFF8A_8A8A);
pub const PILL: Color = Color(0xFF1D_1D1D);
pub const PILL_INK: Color = Color(0xFFF6_F6F6);
pub const HOVER: Color = Color(0x0D00_0000);
pub const TRACK: Color = Color(0x2400_0000);
pub const SURFACE: Color = Color(0xFFF9_F9F9);
pub const HAIR: Color = Color(0x1700_0000);
/// The hairline ring round a floating card.
pub const EDGE: Color = Color(0x1400_0000);

/// Light or dark, as the taskbar is. Every surface is written in the light
/// palette above; the dark theme swaps each grey for its partner at paint
/// time, so layouts and painters never think about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    /// The colour actually painted for a colour of the design.
    pub fn resolve(self, c: Color) -> Color {
        if self == Theme::Light {
            return c;
        }
        // After Windows 11 dark: text F3 / C5 / 9D on a 2C card, the
        // waiting pill turns light, overlays turn from black to white.
        match c {
            INK => Color(0xFFF3_F3F3),
            MUTED => Color(0xFFC5_C5C5),
            TIME => Color(0xFF9D_9D9D),
            PILL => Color(0xFFF0_F0F0),
            PILL_INK => Color(0xFF1D_1D1D),
            HOVER => Color(0x12FF_FFFF),
            TRACK => Color(0x30FF_FFFF),
            SURFACE => Color(0xFF2C_2C2C),
            HAIR => Color(0x17FF_FFFF),
            EDGE => Color(0x1AFF_FFFF),
            other => other,
        }
    }

    /// From the `SystemUsesLightTheme` registry value (0 means a dark
    /// taskbar); light when the value is missing.
    pub fn from_system_value(value: Option<u32>) -> Theme {
        if value == Some(0) { Theme::Dark } else { Theme::Light }
    }
}

/// Opacity of a counter at zero: present, but quiet.
pub const ZERO_OPACITY: f32 = 0.42;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
}

/// Premultiplied BGRA pixels, row by row, top first.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
    pub color: Color,
}

impl TextStyle {
    pub const fn new(size: f32, weight: Weight, color: Color) -> TextStyle {
        TextStyle { size, weight, color }
    }
}

pub struct Renderer {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    family: HSTRING,
    theme: Cell<Theme>,
}

impl Renderer {
    pub fn new() -> Result<Renderer> {
        // SAFETY: plain COM and factory set-up; a second CoInitializeEx on the thread is harmless.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let family = pick_family(&dwrite);
            Ok(Renderer { d2d, dwrite, wic, family, theme: Cell::new(Theme::Light) })
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// Paint from now on in `theme`.
    pub fn set_theme(&self, theme: Theme) {
        self.theme.set(theme);
    }

    /// Draw `w_dip × h_dip` at `scale` physical pixels per DIP.
    pub fn render(&self, w_dip: f32, h_dip: f32, scale: f32, bg: Color, paint: impl FnOnce(&Painter)) -> Result<Frame> {
        let (pw, ph) = ((w_dip * scale).ceil().max(1.0) as u32, (h_dip * scale).ceil().max(1.0) as u32);
        // SAFETY: COM calls on objects created right here; the pixel buffer is sized for the copy.
        unsafe {
            let bitmap = self.wic.CreateBitmap(pw, ph, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * scale,
                dpiY: 96.0 * scale,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = self.d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.BeginDraw();
            rt.Clear(Some(&bg.d2d(1.0)));
            let painter = Painter { r: self, rt: &rt, opacity: Cell::new(1.0) };
            paint(&painter);
            rt.EndDraw(None, None)?;
            let mut pixels = vec![0u32; (pw * ph) as usize];
            let bytes = std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u8>(), pixels.len() * 4);
            bitmap.CopyPixels(std::ptr::null(), pw * 4, bytes)?;
            Ok(Frame { width: pw, height: ph, pixels })
        }
    }

    /// Write a frame as a straight-alpha PNG.
    pub fn save_png(&self, frame: &Frame, path: &Path) -> Result<()> {
        // SAFETY: COM calls on local objects; the source buffer outlives the encoder.
        unsafe {
            let bytes = std::slice::from_raw_parts(frame.pixels.as_ptr().cast::<u8>(), frame.pixels.len() * 4);
            let src = self.wic.CreateBitmapFromMemory(frame.width, frame.height, &GUID_WICPixelFormat32bppPBGRA, frame.width * 4, bytes)?;
            let straight = WICConvertBitmapSource(&GUID_WICPixelFormat32bppBGRA, &src)?;
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let encoder = self.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut target = None;
            let mut options = None;
            encoder.CreateNewFrame(&mut target, &mut options)?;
            let target = target.expect("PNG frame");
            target.Initialize(options.as_ref())?;
            target.SetSize(frame.width, frame.height)?;
            let mut format = GUID_WICPixelFormat32bppBGRA;
            target.SetPixelFormat(&mut format)?;
            target.WriteSource(&straight, std::ptr::null())?;
            target.Commit()?;
            encoder.Commit()
        }
    }

    fn format(&self, style: &TextStyle) -> Result<IDWriteTextFormat> {
        let weight = match style.weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        };
        // SAFETY: string arguments live through the call.
        unsafe {
            self.dwrite.CreateTextFormat(
                &self.family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                style.size,
                w!("ru-ru"),
            )
        }
    }
}

/// Segoe UI Variable Text where the system has it, Segoe UI otherwise.
fn pick_family(dwrite: &IDWriteFactory) -> HSTRING {
    let preferred = HSTRING::from("Segoe UI Variable Text");
    // SAFETY: out parameters are local and initialised.
    let found = unsafe {
        let mut collection = None;
        dwrite.GetSystemFontCollection(&mut collection, false).ok().and(collection).is_some_and(|c| {
            let (mut index, mut exists) = (0u32, Default::default());
            c.FindFamilyName(&preferred, &mut index, &mut exists).is_ok() && exists.as_bool()
        })
    };
    if found { preferred } else { HSTRING::from("Segoe UI") }
}

/// Measured text, ready to draw.
pub struct TextBox {
    layout: IDWriteTextLayout,
    color: Color,
    pub width: f32,
    pub height: f32,
    /// Distance from the top of the box to the baseline.
    pub baseline: f32,
}

pub struct Painter<'a> {
    r: &'a Renderer,
    rt: &'a ID2D1RenderTarget,
    opacity: Cell<f32>,
}

impl Painter<'_> {
    /// Everything drawn inside `f` is drawn at `opacity`.
    pub fn with_opacity(&self, opacity: f32, f: impl FnOnce(&Self)) {
        let before = self.opacity.replace(self.opacity.get() * opacity);
        f(self);
        self.opacity.set(before);
    }

    fn brush(&self, color: Color) -> Option<ID2D1SolidColorBrush> {
        let color = self.r.theme.get().resolve(color);
        // SAFETY: the render target is in a BeginDraw/EndDraw pair.
        unsafe { self.rt.CreateSolidColorBrush(&color.d2d(self.opacity.get()), None).ok() }
    }

    fn round_stroke(&self) -> Option<ID2D1StrokeStyle> {
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
        };
        // SAFETY: factory call with a local struct.
        unsafe { self.r.d2d.CreateStrokeStyle(&props, None).ok() }
    }

    pub fn fill_round(&self, rect: Rect, radius: f32, color: Color) {
        let Some(b) = self.brush(color) else { return };
        let rr = D2D1_ROUNDED_RECT { rect: rect.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.FillRoundedRectangle(&rr, &b) }
    }

    pub fn fill_rect(&self, rect: Rect, color: Color) {
        let Some(b) = self.brush(color) else { return };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.FillRectangle(&rect.d2d(), &b) }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, color: Color) {
        let Some(b) = self.brush(color) else { return };
        let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.FillEllipse(&e, &b) }
    }

    pub fn stroke_circle(&self, cx: f32, cy: f32, r: f32, width: f32, color: Color) {
        let Some(b) = self.brush(color) else { return };
        let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.DrawEllipse(&e, &b, width, None) }
    }

    pub fn stroke_round_rect(&self, rect: Rect, radius: f32, width: f32, color: Color) {
        let Some(b) = self.brush(color) else { return };
        let rr = D2D1_ROUNDED_RECT { rect: rect.d2d(), radiusX: radius, radiusY: radius };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.DrawRoundedRectangle(&rr, &b, width, None) }
    }

    /// A line with round caps.
    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Color) {
        let Some(b) = self.brush(color) else { return };
        let style = self.round_stroke();
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.DrawLine(pt(x1, y1), pt(x2, y2), &b, width, style.as_ref()) }
    }

    /// A polyline with round caps and joins.
    pub fn polyline(&self, points: &[(f32, f32)], width: f32, color: Color) {
        let (Some(b), Some(&(x0, y0))) = (self.brush(color), points.first()) else { return };
        // SAFETY: geometry built and drawn inside BeginDraw/EndDraw.
        unsafe {
            let Ok(geo) = self.r.d2d.CreatePathGeometry() else { return };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(pt(x0, y0), D2D1_FIGURE_BEGIN_HOLLOW);
            for &(x, y) in &points[1..] {
                sink.AddLine(pt(x, y));
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.rt.DrawGeometry(&geo, &b, width, self.round_stroke().as_ref());
            }
        }
    }

    /// A ring: the track all round, the arc from twelve o'clock clockwise for `frac`.
    #[allow(clippy::too_many_arguments, reason = "a drawing primitive: centre, radius, stroke, value, two colours")]
    pub fn ring(&self, cx: f32, cy: f32, r: f32, width: f32, frac: f32, color: Color, track: Color) {
        self.stroke_circle(cx, cy, r, width, track);
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.0 {
            return;
        }
        if frac >= 0.999 {
            self.stroke_circle(cx, cy, r, width, color);
            return;
        }
        let Some(b) = self.brush(color) else { return };
        let angle = frac * std::f32::consts::TAU;
        let end = pt(cx + r * angle.sin(), cy - r * angle.cos());
        // SAFETY: geometry built and drawn inside BeginDraw/EndDraw.
        unsafe {
            let Ok(geo) = self.r.d2d.CreatePathGeometry() else { return };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(pt(cx, cy - r), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: end,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.rt.DrawGeometry(&geo, &b, width, None);
            }
        }
    }

    /// A thin progress bar with rounded ends.
    pub fn bar(&self, rect: Rect, frac: f32, color: Color, track: Color) {
        let radius = rect.h / 2.0;
        self.fill_round(rect, radius, track);
        let w = rect.w * frac.clamp(0.0, 1.0);
        if w > 0.0 {
            self.fill_round(Rect { w: w.max(rect.h), ..rect }, radius, color);
        }
    }

    /// Everything drawn inside `f` is shifted by (`dx`, `dy`): lets a window's
    /// layout be drawn into a larger scene for `--render`.
    pub fn translate(&self, dx: f32, dy: f32, f: impl FnOnce(&Self)) {
        use windows::Foundation::Numerics::Matrix3x2;
        let mut before = Matrix3x2::default();
        // SAFETY: transform get/set paired around `f`, inside BeginDraw/EndDraw.
        unsafe {
            self.rt.GetTransform(&mut before);
            let mut moved = before;
            moved.M31 += dx;
            moved.M32 += dy;
            self.rt.SetTransform(&moved);
        }
        f(self);
        // SAFETY: restores the transform saved above.
        unsafe { self.rt.SetTransform(&before) };
    }

    /// Everything drawn inside `f` is clipped to `rect`.
    pub fn clip(&self, rect: Rect, f: impl FnOnce(&Self)) {
        // SAFETY: push and pop are paired around `f`, inside BeginDraw/EndDraw.
        unsafe { self.rt.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        f(self);
        // SAFETY: pops the clip pushed above.
        unsafe { self.rt.PopAxisAlignedClip() };
    }

    /// A soft drop shadow and a hairline edge under a floating card, after
    /// Windows 11 flyouts: `0 8px 28px rgba(0,0,0,.16)` plus a 1 px ring.
    pub fn card_shadow(&self, card: Rect, radius: f32) {
        const LAYERS: usize = 14;
        const SPREAD: f32 = 28.0;
        const DROP: f32 = 8.0;
        for i in 0..LAYERS {
            let grow = SPREAD * (1.0 - i as f32 / LAYERS as f32);
            let r = Rect::new(card.x - grow / 2.0, card.y + DROP - grow / 2.0, card.w + grow, card.h + grow);
            self.fill_round(r, radius + grow / 2.0, Color(0x0300_0000));
        }
        let ring = Rect::new(card.x - 0.5, card.y - 0.5, card.w + 1.0, card.h + 1.0);
        self.stroke_round_rect(ring, radius + 0.5, 1.0, EDGE);
    }

    /// Lay out `text` on one line (ellipsis past `max_w`) or wrapped (`wrap`).
    pub fn text_box(&self, text: &str, style: &TextStyle, max_w: f32, wrap: bool) -> Option<TextBox> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: DirectWrite calls with buffers that live through each call.
        unsafe {
            let format = self.r.format(style).ok()?;
            if !wrap {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
                let sign = self.r.dwrite.CreateEllipsisTrimmingSign(&format).ok()?;
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                format.SetTrimming(&trimming, &sign).ok()?;
            }
            let layout = self.r.dwrite.CreateTextLayout(&wide, &format, max_w, 10_000.0).ok()?;
            let typography = self.r.dwrite.CreateTypography().ok()?;
            typography.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 }).ok()?;
            layout.SetTypography(&typography, DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 }).ok()?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m).ok()?;
            let mut line = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0u32;
            let _ = layout.GetLineMetrics(Some(&mut line), &mut count);
            Some(TextBox { layout, color: style.color, width: m.width.min(max_w), height: m.height, baseline: line[0].baseline })
        }
    }

    /// One line of text; returns its width. `y` is the top of the line box.
    pub fn text(&self, text: &str, x: f32, y: f32, style: &TextStyle) -> f32 {
        self.text_box(text, style, 10_000.0, false).map_or(0.0, |b| {
            self.draw_box(&b, x, y);
            b.width
        })
    }

    pub fn measure(&self, text: &str, style: &TextStyle) -> (f32, f32) {
        self.text_box(text, style, 10_000.0, false).map_or((0.0, 0.0), |b| (b.width, b.height))
    }

    pub fn draw_box(&self, b: &TextBox, x: f32, y: f32) {
        let Some(brush) = self.brush(b.color) else { return };
        // SAFETY: drawing inside BeginDraw/EndDraw.
        unsafe { self.rt.DrawTextLayout(pt(x, y), &b.layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE) }
    }

    /// Draw a PNG from disk into `dest` (used for mock backgrounds in `--render`).
    pub fn image(&self, path: &Path, dest: Rect) {
        // SAFETY: WIC and D2D calls on local objects inside BeginDraw/EndDraw.
        unsafe {
            let Ok(decoder) = self.r.wic.CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            ) else {
                return;
            };
            let Ok(frame) = decoder.GetFrame(0) else { return };
            let Ok(source) = WICConvertBitmapSource(&GUID_WICPixelFormat32bppPBGRA, &frame) else { return };
            let Ok(bitmap) = self.rt.CreateBitmapFromWicBitmap(&source, None) else { return };
            let size = bitmap.GetSize();
            let src = D2D_RECT_F { left: 0.0, top: 0.0, right: dest.w.min(size.width), bottom: dest.h.min(size.height) };
            let dst = Rect { w: src.right, h: src.bottom, ..dest };
            self.rt.DrawBitmap(&bitmap, Some(&dst.d2d()), self.opacity.get(), D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&src));
        }
    }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

