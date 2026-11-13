//! Top-level layered windows above the strip: the sessions window, the toast
//! stack and the tooltip. Corners and the shadow are drawn into the frame,
//! since a layered window gets no DWM shadow; the shadow's transparent
//! margins let clicks through.

use super::{Surface, layered, surface_message};
use crate::render::Frame;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("claudibeat.popup");
const SURFACES: [Surface; 4] = [Surface::Strip, Surface::Flyout, Surface::Toasts, Surface::Tooltip];

fn surface_of(hwnd: HWND) -> Option<Surface> {
    // SAFETY: reads a value we stored on our own window.
    let i = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    SURFACES.get((i as usize).checked_sub(1)?).copied()
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = surface_of(hwnd).and_then(|surface| surface_message(surface, hwnd, msg, wp, lp)) {
        return r;
    }
    // SAFETY: default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub fn register(instance: HINSTANCE) {
    let wc = WNDCLASSW { lpfnWndProc: Some(popup_proc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
    // SAFETY: registers a class whose name and procedure are 'static.
    unsafe { RegisterClassW(&wc) };
}

pub struct Popup {
    hwnd: HWND,
    surface: Surface,
    visible: bool,
}

impl Popup {
    /// The sessions window takes focus (for Esc and closing on a click
    /// elsewhere); toasts never do; the tooltip ignores the mouse entirely.
    pub fn new(instance: HINSTANCE, surface: Surface) -> Option<Popup> {
        let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        if surface != Surface::Flyout {
            ex |= WS_EX_NOACTIVATE;
        }
        if surface == Surface::Tooltip {
            ex |= WS_EX_TRANSPARENT;
        }
        // SAFETY: creates our own window and tags it with its surface.
        unsafe {
            let hwnd = CreateWindowExW(ex, CLASS, w!("claudibeat"), WS_POPUP, 0, 0, 1, 1, None, None, instance, None).ok()?;
            let index = SURFACES.iter().position(|&s| s == surface).unwrap_or(0) + 1;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, index as isize);
            Some(Popup { hwnd, surface, visible: false })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Show `frame` with its top-left corner at screen point (`x`, `y`).
    pub fn show(&mut self, frame: &Frame, x: i32, y: i32) -> windows::core::Result<()> {
        layered::present(self.hwnd, frame, Some(POINT { x, y }), false)?;
        if !self.visible {
            self.visible = true;
            // SAFETY: z-order and visibility of our own window.
            unsafe {
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                if self.surface == Surface::Flyout {
                    let _ = SetForegroundWindow(self.hwnd);
                }
            }
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.visible {
            self.visible = false;
            // SAFETY: hides our own window.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: destroys our own window.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
