//! Opening chats and starting with Windows.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL};
use windows::core::{HSTRING, w};

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("claudibeat");

/// Hand a `claude://` link to the Claude app. Callers pass only links that
/// `merge::open_target` approved.
pub fn open_url(url: &str) -> bool {
    // SAFETY: strings live through the call. We just took the person's
    // click, so we may pass the right to come forward on to Claude.
    let r = unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL)
    };
    r.0 as isize > 32
}

/// True while a full-screen game, video or presentation owns the screen.
pub fn is_busy() -> bool {
    // SAFETY: a plain query.
    let state: QUERY_USER_NOTIFICATION_STATE = unsafe { SHQueryUserNotificationState() }.unwrap_or_default();
    state == QUNS_BUSY || state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE
}

pub fn autostart_get() -> bool {
    // SAFETY: queries the size of one registry value.
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}

pub fn autostart_set(on: bool) {
    // SAFETY: opens, writes or deletes one value under the user's Run key.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let text = format!("\"{}\"", exe.display());
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                let _ = RegSetValueExW(key, VALUE, 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, VALUE);
        }
        let _ = RegCloseKey(key);
    }
}
