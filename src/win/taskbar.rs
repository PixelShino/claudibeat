//! Living inside the taskbar as a child window of `Shell_TrayWnd`.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// The primary monitor's taskbar.
pub fn primary() -> Option<HWND> {
    // SAFETY: a lookup by class name.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None).ok().filter(|h| !h.is_invalid()) }
}

/// Physical pixels per DIP on the taskbar's monitor.
pub fn scale(taskbar: HWND) -> f32 {
    // SAFETY: a query on a live window handle.
    let dpi = unsafe { GetDpiForWindow(taskbar) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// Height of the taskbar's client area in physical pixels.
pub fn height(taskbar: HWND) -> i32 {
    let mut rect = RECT::default();
    // SAFETY: out parameter is local.
    let _ = unsafe { GetClientRect(taskbar, &mut rect) };
    rect.bottom - rect.top
}

/// Make `hwnd` a child of the taskbar.
///
/// After `SetParent`, Windows 11 can leave a layered surface under the
/// taskbar's own DirectComposition visual; toggling `WS_EX_LAYERED` once
/// rebinds it (as CodeZeno's `embed_as_child` does).
pub fn embed(hwnd: HWND, taskbar: HWND) {
    // SAFETY: style changes and reparenting on our own window.
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let ex = (ex | (WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE).0 as i32) & !(WS_EX_TOPMOST.0 as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let style = (style & !WS_POPUP.0) | WS_CHILD.0 | WS_CLIPSIBLINGS.0;
        SetWindowLongW(hwnd, GWL_STYLE, style as i32);
        if GetParent(hwnd).ok() != Some(taskbar) {
            let _ = SetParent(hwnd, taskbar);
            if GetParent(hwnd).ok() == Some(taskbar) {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as i32));
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
            }
        }
        let _ = SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// Put the strip at the taskbar's left edge, full height.
pub fn place_left(hwnd: HWND, taskbar: HWND, width_px: i32) {
    let h = height(taskbar);
    // SAFETY: positioning our own child window.
    let _ = unsafe { SetWindowPos(hwnd, HWND_TOP, 0, 0, width_px, h, SWP_NOACTIVATE | SWP_SHOWWINDOW) };
}

/// The message Explorer broadcasts when it recreates the taskbar.
pub fn created_msg() -> u32 {
    // SAFETY: registers or looks up a global message name.
    unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
}
