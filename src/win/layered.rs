//! Putting a rendered frame on screen through `UpdateLayeredWindow`.
//!
//! The technique follows CodeZeno's Claude-Code-Usage-Monitor (MIT), which
//! proved it on Windows 11 taskbars: a premultiplied 32-bit DIB, a screen DC
//! as destination, and (for the strip) fully transparent pixels nudged to
//! alpha 1 so the whole surface still takes the mouse.

use crate::render::Frame;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};

/// Show `frame` in the layered window `hwnd`, moved to `pos` if given.
/// With `catch_all`, transparent pixels still take the mouse; without it,
/// clicks fall through them (popups' shadow margins).
pub fn present(hwnd: HWND, frame: &Frame, pos: Option<POINT>, catch_all: bool) -> windows::core::Result<()> {
    if frame.width == 0 || frame.height == 0 || frame.pixels.len() != frame.width as usize * frame.height as usize {
        return Err(windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG));
    }
    // SAFETY: GDI objects are created, selected, used and released in order;
    // the DIB holds width x height pixels, which the check above makes equal
    // to the frame's own pixel count.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: frame.width as i32,
                biHeight: -(frame.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(memory, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) if !bits.is_null() => {
                let old = SelectObject(memory, bitmap);
                let target = std::slice::from_raw_parts_mut(bits.cast::<u32>(), frame.pixels.len());
                for (t, &s) in target.iter_mut().zip(&frame.pixels) {
                    *t = if catch_all && s >> 24 == 0 { 0x0100_0000 } else { s };
                }
                let size = SIZE { cx: frame.width as i32, cy: frame.height as i32 };
                let origin = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
                let r = UpdateLayeredWindow(
                    hwnd,
                    screen,
                    pos.as_ref().map(|p| p as *const POINT),
                    Some(&size),
                    memory,
                    Some(&origin),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
                SelectObject(memory, old);
                let _ = DeleteObject(bitmap);
                r
            }
            Ok(bitmap) => {
                let _ = DeleteObject(bitmap);
                Err(windows::core::Error::from_win32())
            }
            Err(e) => Err(e),
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}
