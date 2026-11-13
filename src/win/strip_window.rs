//! The strip's own window: a layered child of the taskbar, and its menu.

use super::{Action, Surface, layered, surface_message, taskbar};
use crate::render::Frame;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub const CLASS: PCWSTR = w!("claudibeat.strip");

unsafe extern "system" fn strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = surface_message(Surface::Strip, hwnd, msg, wp, lp) {
        return r;
    }
    // SAFETY: default handling for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub fn register(instance: HINSTANCE) {
    let wc = WNDCLASSW { lpfnWndProc: Some(strip_proc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
    // SAFETY: registers a class whose name and procedure are 'static.
    unsafe { RegisterClassW(&wc) };
}

/// The strip window. Explorer destroys its taskbar's children when it
/// restarts, so the window is re-created whenever the taskbar is new.
#[derive(Default)]
pub struct StripWindow {
    hwnd: Option<HWND>,
}

impl StripWindow {
    /// The live window and its taskbar, creating and embedding it if needed.
    pub fn ensure(&mut self, instance: HINSTANCE) -> Option<(HWND, HWND)> {
        let bar = taskbar::primary()?;
        // SAFETY: queries on window handles, and creation of our own window.
        unsafe {
            if let Some(h) = self.hwnd {
                if IsWindow(h).as_bool() && GetParent(h).ok() == Some(bar) {
                    return Some((h, bar));
                }
                if IsWindow(h).as_bool() {
                    let _ = DestroyWindow(h);
                }
            }
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("claudibeat"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                None,
            )
            .ok()?;
            taskbar::embed(hwnd, bar);
            self.hwnd = Some(hwnd);
            Some((hwnd, bar))
        }
    }

    /// Put `frame` at the taskbar's left edge.
    pub fn show(&self, hwnd: HWND, bar: HWND, frame: &Frame) -> windows::core::Result<()> {
        taskbar::place_left(hwnd, bar, frame.width as i32);
        layered::present(hwnd, frame, None, true)
    }

    /// Explorer made a new taskbar: the old window is gone or orphaned.
    pub fn forget(&mut self) {
        if let Some(h) = self.hwnd.take() {
            // SAFETY: destroying our own window if it still exists.
            unsafe {
                if IsWindow(h).as_bool() {
                    let _ = DestroyWindow(h);
                }
            }
        }
    }
}

impl Drop for StripWindow {
    fn drop(&mut self) {
        self.forget();
    }
}

const ID_SEEN: usize = 1;
const ID_SOUND: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_QUIT: usize = 4;

/// The right-click menu at the pointer. Returns the chosen action.
pub fn menu(owner: HWND, anything_pending: bool, sound: bool, autostart: bool) -> Option<Action> {
    let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    // SAFETY: a menu created, shown modally and destroyed here.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let seen = if anything_pending { MF_ENABLED } else { MF_GRAYED };
        let _ = AppendMenuW(menu, MF_STRING | seen, ID_SEEN, w!("Отметить всё просмотренным"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING | check(sound), ID_SOUND, w!("Звук уведомлений"));
        let _ = AppendMenuW(menu, MF_STRING | check(autostart), ID_AUTOSTART, w!("Запускать вместе с Windows"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Выход"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // The owner must be foreground, or the menu will not close when the
        // person clicks elsewhere (the documented TrackPopupMenu caveat).
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_NONOTIFY;
        let id = TrackPopupMenu(menu, flags, pt.x, pt.y, 0, owner, None).0 as usize;
        let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match id {
            ID_SEEN => Some(Action::MarkAllSeen),
            ID_SOUND => Some(Action::ToggleSound),
            ID_AUTOSTART => Some(Action::ToggleAutostart),
            ID_QUIT => Some(Action::Quit),
            _ => None,
        }
    }
}
