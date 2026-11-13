use claudibeat::render::{Color, EDGE, HAIR, HOVER, INK, MUTED, PILL, PILL_INK, Rect, Renderer, SURFACE, TIME, TRACK, TRANSPARENT, Theme};

const PALETTE: [Color; 10] = [INK, MUTED, TIME, PILL, PILL_INK, HOVER, TRACK, SURFACE, HAIR, EDGE];

/// Windows 11's dark taskbar, near enough.
const DARK_BAR: u32 = 0x1C1C1C;

/// `c` (straight alpha) laid over an opaque `bg`, as `0xRRGGBB`.
fn over(c: Color, bg: u32) -> u32 {
    let a = (c.0 >> 24) as f64 / 255.0;
    let mix = |s: u32| {
        let (f, b) = (((c.0 >> s) & 0xFF) as f64, ((bg >> s) & 0xFF) as f64);
        ((f * a + b * (1.0 - a)).round() as u32) << s
    };
    mix(16) | mix(8) | mix(0)
}

fn luminance(rgb: u32) -> f64 {
    let lin = |s: u32| {
        let v = ((rgb >> s) & 0xFF) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
}

fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn the_light_theme_paints_the_design_as_written() {
    for c in PALETTE {
        assert_eq!(Theme::Light.resolve(c), c);
    }
    assert_eq!(Theme::default(), Theme::Light);
}

#[test]
fn every_grey_has_a_dark_partner() {
    for c in PALETTE {
        assert_ne!(Theme::Dark.resolve(c), c, "{c:?} stays the same in the dark theme");
    }
    assert_eq!(Theme::Dark.resolve(TRANSPARENT), TRANSPARENT);
}

#[test]
fn the_dark_theme_reads_on_a_dark_taskbar_and_card() {
    let d = |c| Theme::Dark.resolve(c);
    let ink_on_bar = contrast(over(d(INK), DARK_BAR), DARK_BAR);
    assert!(ink_on_bar >= 7.0, "strip text on the dark taskbar: {ink_on_bar:.1}");
    let pill_on_bar = contrast(over(d(PILL), DARK_BAR), DARK_BAR);
    assert!(pill_on_bar >= 7.0, "the waiting pill stands out: {pill_on_bar:.1}");

    let card = over(d(SURFACE), DARK_BAR);
    assert!(luminance(card) < 0.04, "the card itself is dark");
    for (name, c, min) in [("ink", INK, 7.0), ("muted", MUTED, 4.5), ("time", TIME, 3.0)] {
        let ratio = contrast(over(d(c), card), card);
        assert!(ratio >= min, "{name} on the card: {ratio:.1} < {min}");
    }
    let pill = over(d(PILL), card);
    let ratio = contrast(over(d(PILL_INK), pill), pill);
    assert!(ratio >= 7.0, "text on the pill: {ratio:.1}");
    for (name, c) in [("hover", HOVER), ("track", TRACK), ("hair", HAIR), ("edge", EDGE)] {
        assert_ne!(over(d(c), card), card, "{name} is visible on the card");
    }
}

#[test]
fn the_system_setting_picks_the_theme() {
    // HKCU\...\Themes\Personalize\SystemUsesLightTheme: 0 means a dark taskbar.
    assert_eq!(Theme::from_system_value(Some(0)), Theme::Dark);
    assert_eq!(Theme::from_system_value(Some(1)), Theme::Light);
    assert_eq!(Theme::from_system_value(None), Theme::Light);
}

#[test]
fn the_renderer_paints_in_its_theme() {
    let r = Renderer::new().unwrap();
    let paint = |r: &Renderer| r.render(4.0, 4.0, 1.0, TRANSPARENT, |p| p.fill_rect(Rect::new(0.0, 0.0, 4.0, 4.0), INK)).unwrap().pixels[5];
    let light = paint(&r);
    r.set_theme(Theme::Dark);
    assert_eq!(r.theme(), Theme::Dark);
    let dark = paint(&r);
    assert!((light & 0xFF) < 0x40, "light theme ink is dark: {light:08X}");
    assert!((dark & 0xFF) > 0xC0, "dark theme ink is light: {dark:08X}");
}
