//! Opening chats, starting with Windows, and the taskbar's colour mode.

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

/// The scheduled task that starts claudibeat at sign-in.
const TASK: &str = "claudibeat";

/// Task Scheduler XML for that task: at this person's sign-in, on their
/// own desktop without elevation, never stopped, at normal priority.
pub fn task_xml(user: &str, exe: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let (user, exe) = (esc(user), esc(exe));
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n",
            "<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n",
            "  <RegistrationInfo><Description>claudibeat: Claude sessions on the taskbar</Description></RegistrationInfo>\n",
            "  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT5S</Delay></LogonTrigger></Triggers>\n",
            "  <Principals><Principal id=\"Author\"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n",
            "  <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>4</Priority></Settings>\n",
            "  <Actions Context=\"Author\"><Exec><Command>{exe}</Command></Exec></Actions>\n",
            "</Task>\n",
        ),
        user = user,
        exe = exe,
    )
}

/// Run `schtasks.exe` from System32 without a console window; true on success.
fn schtasks(args: &[&std::ffi::OsStr]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::var_os("SystemRoot").map_or_else(|| "schtasks.exe".into(), |root| std::path::Path::new(&root).join("System32").join("schtasks.exe"));
    std::process::Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether claudibeat starts at sign-in. Spawns `schtasks`, so callers cache it.
pub fn autostart_get() -> bool {
    schtasks(&["/Query".as_ref(), "/TN".as_ref(), TASK.as_ref()])
}

/// Start claudibeat at sign-in through Task Scheduler, or stop doing so.
///
/// Not the `Run` key: on one Windows 11 machine Explorer skipped the first
/// entries of `HKCU\...\Run` at every sign-in, claudibeat among them, with
/// nothing in any log. A logon task does not depend on Explorer.
pub fn autostart_set(on: bool) {
    if !on {
        schtasks(&["/Delete".as_ref(), "/TN".as_ref(), TASK.as_ref(), "/F".as_ref()]);
        remove_run_value();
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
    let xml = task_xml(&user, &exe.display().to_string());
    // schtasks reads the file in the encoding its declaration names: UTF-16 with a BOM.
    let bytes: Vec<u8> = std::iter::once(0xFEFF).chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    let path = std::env::temp_dir().join("claudibeat-task.xml");
    if std::fs::write(&path, bytes).is_err() {
        return;
    }
    let created = schtasks(&["/Create".as_ref(), "/TN".as_ref(), TASK.as_ref(), "/XML".as_ref(), path.as_os_str(), "/F".as_ref()]);
    let _ = std::fs::remove_file(&path);
    if created {
        remove_run_value();
    }
}

/// Builds before the logon task started through the `Run` key. Carry that
/// choice over once; the `Run` value goes only when the task is in place.
pub fn migrate_autostart() {
    // SAFETY: queries the size of one registry value.
    let in_run = unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS };
    if in_run {
        autostart_set(true);
    }
}

fn remove_run_value() {
    // SAFETY: opens the user's Run key and deletes one value from it.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key) == ERROR_SUCCESS {
            let _ = RegDeleteValueW(key, VALUE);
            let _ = RegCloseKey(key);
        }
    }
}

/// Light or dark, as Windows paints the taskbar ("Choose your default
/// Windows mode"). The strip sits on the taskbar and the popups belong to
/// the shell, so they all follow this rather than the apps' mode.
pub fn system_theme() -> crate::render::Theme {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reads one DWORD into a local of the declared size.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    crate::render::Theme::from_system_value((read == ERROR_SUCCESS).then_some(value))
}
