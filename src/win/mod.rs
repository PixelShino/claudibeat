//! Win32 windows and shell plumbing.
//!
//! Window procedures never touch the application's state. They turn mouse
//! and keyboard messages into [`Input`]s, queue them and post a note to the
//! host window, which hands them to the app outside any Win32 call. That way
//! a call such as `SetWindowPos` or a menu's modal loop, which re-enters
//! window procedures, can never meet the app's state already borrowed.

pub mod layered;
pub mod popup;
pub mod shell;
pub mod strip_window;
pub mod taskbar;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

/// From `Win32_UI_Controls`, not worth the whole feature for one number.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Posted to the host when inputs are waiting in the queue.
pub const WM_INPUT_READY: u32 = WM_APP + 1;
/// Posted to the host by the folder watcher.
pub const WM_FOLDER_CHANGED: u32 = WM_APP + 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Strip,
    Flyout,
    Toasts,
    Tooltip,
}

impl Surface {
    fn index(self) -> usize {
        self as usize
    }
}

/// What a person did to one of the windows. Points are in the window's
/// physical pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Move(Surface, i32, i32),
    Leave(Surface),
    Click(Surface, i32, i32),
    RightClick(Surface),
    /// Wheel delta, 120 per notch, positive away from the person.
    Wheel(Surface, i32),
    Escape(Surface),
    Deactivated(Surface),
}

/// What the app should do in answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    OpenChat(String),
    OpenFlyout,
    ToggleFlyout,
    MarkAllSeen,
    ToggleSound,
    ToggleAutostart,
    ToggleDnd,
    SetLanguage(crate::text::Lang),
    Quit,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Input>> = const { RefCell::new(VecDeque::new()) };
    static HOST: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static HAND: Cell<[bool; 4]> = const { Cell::new([false; 4]) };
    /// A click needs its press on the same window: releasing a drag from a
    /// taskbar button over the strip is not a click.
    static PRESSED: Cell<[bool; 4]> = const { Cell::new([false; 4]) };
}

pub fn set_host(hwnd: HWND) {
    HOST.set(hwnd);
}

/// Queue an input for the app and wake the host.
pub fn emit(input: Input) {
    QUEUE.with_borrow_mut(|q| q.push_back(input));
    // SAFETY: posting to our own window.
    let _ = unsafe { PostMessageW(HOST.get(), WM_INPUT_READY, WPARAM(0), LPARAM(0)) };
}

pub fn next_input() -> Option<Input> {
    QUEUE.with_borrow_mut(|q| q.pop_front())
}

/// Show the hand cursor over `surface` while the pointer is on something
/// clickable.
pub fn set_hand(surface: Surface, on: bool) {
    let mut h = HAND.get();
    h[surface.index()] = on;
    HAND.set(h);
}

fn point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

/// Shared handling for every claudibeat window: the messages that become
/// inputs. `None` means the caller should fall through to its own handling.
pub(crate) fn surface_message(surface: Surface, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_MOUSEMOVE => {
            let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            // SAFETY: tracking our own window.
            let _ = unsafe { TrackMouseEvent(&mut tme) };
            let (x, y) = point(lp);
            emit(Input::Move(surface, x, y));
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            let mut p = PRESSED.get();
            p[surface.index()] = false;
            PRESSED.set(p);
            emit(Input::Leave(surface));
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            let mut p = PRESSED.get();
            p[surface.index()] = true;
            PRESSED.set(p);
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            let mut p = PRESSED.get();
            let was = std::mem::replace(&mut p[surface.index()], false);
            PRESSED.set(p);
            if was {
                let (x, y) = point(lp);
                emit(Input::Click(surface, x, y));
            }
            Some(LRESULT(0))
        }
        WM_RBUTTONUP => {
            emit(Input::RightClick(surface));
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL => {
            emit(Input::Wheel(surface, ((wp.0 >> 16) & 0xFFFF) as i16 as i32));
            Some(LRESULT(0))
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            emit(Input::Escape(surface));
            Some(LRESULT(0))
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            emit(Input::Deactivated(surface));
            Some(LRESULT(0))
        }
        // Alt+F4 closes the window it is pressed in, as losing focus does;
        // answering it here keeps it from turning into a WM_CLOSE.
        WM_SYSCOMMAND if (wp.0 & 0xFFF0) as u32 == SC_CLOSE => {
            emit(Input::Deactivated(surface));
            Some(LRESULT(0))
        }
        // A WM_CLOSE from outside (taskkill without /F picks any one of our
        // top-level windows) means "quit": leave as the menu does, instead
        // of letting DefWindowProc destroy a popup under the app.
        WM_CLOSE => {
            // SAFETY: ends this thread's message loop; every window is ours.
            unsafe { PostQuitMessage(0) };
            Some(LRESULT(0))
        }
        // Clicking the strip or a toast must not take focus from the
        // person's own window; only the sessions window activates itself.
        WM_MOUSEACTIVATE if surface != Surface::Flyout => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            let id = if HAND.get()[surface.index()] { IDC_HAND } else { IDC_ARROW };
            // SAFETY: loads a shared system cursor.
            unsafe { SetCursor(LoadCursorW(None, id).unwrap_or_default()) };
            Some(LRESULT(1))
        }
        _ => None,
    }
}

/// Where the pointer is inside `hwnd` (client pixels), if it is over that
/// window right now. Mouse-leave messages can go missing when a window
/// changes under a still pointer; this is the ground truth.
pub fn cursor_over(hwnd: HWND) -> Option<(i32, i32)> {
    // SAFETY: plain queries with local out parameters.
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        if WindowFromPoint(pt) != hwnd {
            return None;
        }
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
        Some((pt.x, pt.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Take a pending `WM_QUIT` off this thread's queue, if there is one.
    fn quit_pending() -> bool {
        let mut msg = MSG::default();
        // SAFETY: peeks at the calling thread's own queue.
        unsafe { PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool() }
    }

    #[test]
    fn a_close_request_to_any_window_quits_the_app() {
        // taskkill without /F picks one of our top-level windows, not
        // necessarily the host: a popup must not just be destroyed.
        while quit_pending() {}
        let answer = surface_message(Surface::Toasts, HWND::default(), WM_CLOSE, WPARAM(0), LPARAM(0));
        assert_eq!(answer, Some(LRESULT(0)), "handled, so DefWindowProc never destroys the popup");
        assert!(quit_pending(), "the message loop is told to end");
    }

    #[test]
    fn alt_f4_closes_the_sessions_window_not_the_app() {
        while quit_pending() {}
        while next_input().is_some() {}
        let answer = surface_message(Surface::Flyout, HWND::default(), WM_SYSCOMMAND, WPARAM(SC_CLOSE as usize), LPARAM(0));
        assert_eq!(answer, Some(LRESULT(0)), "no WM_CLOSE follows");
        assert_eq!(next_input(), Some(Input::Deactivated(Surface::Flyout)));
        assert!(!quit_pending());
    }
}
