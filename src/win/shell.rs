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
/// Whether the window in front belongs to Claude Desktop (`claude.exe`).
pub fn claude_in_front() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries with local buffers; the process handle is closed.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok && String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase().ends_with(r"\claude.exe")
    }
}

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

/// Start the sign-in task now: claudibeat then runs as Task Scheduler starts
/// it, outside whatever started this process.
pub fn run_task() -> bool {
    schtasks(&["/Run".as_ref(), "/TN".as_ref(), TASK.as_ref()])
}

/// The full name of the app package this process runs inside, if any.
///
/// Claude Desktop is an MSIX package: a program its chats start runs inside
/// it, its AppData writes go to the package's own copy, and it is ended with
/// the app when Claude restarts or updates. claudibeat must not live there.
pub fn package_name() -> Option<String> {
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut len = 0u32;
    // SAFETY: a size query, then a call with a buffer of that size.
    unsafe {
        let _ = GetCurrentPackageFullName(&mut len, windows::core::PWSTR::null());
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        if GetCurrentPackageFullName(&mut len, windows::core::PWSTR(buf.as_mut_ptr())).is_err() {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]))
    }
}

/// The Claude app among this process's ancestors, if one is: a program a
/// Claude chat starts lives under it and is ended when Claude restarts.
pub fn started_under_claude() -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    let mut table: Vec<(u32, u32, String)> = Vec::new();
    // SAFETY: a process snapshot walked with a local entry, then closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let end = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                table.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..end])));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    ancestor_named(&table, std::process::id(), "claude.exe")
}

/// Process ancestry ends at these: a parent id above them is long dead and
/// may name an unrelated process by now (Windows reuses ids).
const ROOTS: [&str; 5] = ["svchost.exe", "services.exe", "wininit.exe", "winlogon.exe", "explorer.exe"];

/// Walk up from `pid` through (pid, parent, name) rows to a process named
/// `name`; a cycle, 32 steps or a system root ([`ROOTS`]) end the walk.
pub fn ancestor_named(table: &[(u32, u32, String)], pid: u32, name: &str) -> Option<String> {
    let mut at = table.iter().find(|r| r.0 == pid)?.1;
    let mut path = Vec::new();
    for _ in 0..32 {
        let row = table.iter().find(|r| r.0 == at)?;
        if row.2.eq_ignore_ascii_case(name) {
            path.push(format!("{} (pid {})", row.2, row.0));
            return Some(path.into_iter().rev().collect::<Vec<_>>().join(" > "));
        }
        if ROOTS.iter().any(|r| row.2.eq_ignore_ascii_case(r)) {
            return None;
        }
        path.push(row.2.clone());
        if row.1 == row.0 || row.1 == 0 {
            return None;
        }
        at = row.1;
    }
    None
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

/// A folder of the signed-in user (`FOLDERID_RoamingAppData`, `FOLDERID_Profile`),
/// asked of Windows itself: a process the Task Scheduler starts need not have
/// `APPDATA` or `USERPROFILE` in its environment.
pub fn known_folder(id: &windows::core::GUID) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned string is copied, then freed with the allocator Windows names.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// `%APPDATA%`: from Windows, the environment, or the exe's own place.
pub fn roaming_dir() -> std::path::PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_RoamingAppData)
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
        .filter(|p| p.is_absolute())
        .or_else(|| profile_from_exe().map(|h| h.join("AppData").join("Roaming")))
        .unwrap_or_default()
}

/// `%USERPROFILE%`: from Windows, the environment, or the exe's own place.
pub fn profile_dir() -> std::path::PathBuf {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Profile)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .filter(|p| p.is_absolute())
        .or_else(profile_from_exe)
        .unwrap_or_default()
}

/// The profile the exe is installed in: the folder holding `AppData` on the
/// way up from `<profile>\AppData\Local\Programs\claudibeat\claudibeat.exe`.
pub fn profile_from_exe() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut p = exe.as_path();
    while let Some(parent) = p.parent() {
        if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("AppData")) {
            return Some(parent.to_path_buf());
        }
        p = parent;
    }
    None
}

/// Where the folders came from, for the log.
pub fn folder_sources() -> String {
    let win = known_folder(&windows::Win32::UI::Shell::FOLDERID_RoamingAppData).is_some();
    let env = std::env::var_os("APPDATA").is_some();
    format!("known folder: {win}, APPDATA: {env}, exe profile: {:?}", profile_from_exe())
}
