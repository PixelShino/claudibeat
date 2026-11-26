//! Staying up, and saying why not.
//!
//! The release build aborts on a panic (`panic = "abort"`), which leaves no
//! trace anywhere: no Windows event, no report. So every panic and every exit
//! is written to `%USERPROFILE%\.claude\claudibeat\claudibeat.log`, and a small guard
//! process runs the strip as its child and starts it again if it dies.
//! A quit that was asked for (the menu, `--quit`, a polite `taskkill`) ends
//! the child with code 0, and the guard leaves with it.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The flag the guard starts its child with.
pub const CHILD_FLAG: &str = "--child";
/// More restarts than this within [`WINDOW`] means something is badly wrong:
/// the guard stops trying and says so in the log.
const MAX_RESTARTS: usize = 5;
const WINDOW: Duration = Duration::from_secs(600);
const PAUSE: Duration = Duration::from_secs(2);
/// The log keeps its tail when it grows past this.
const LOG_MAX: u64 = 400 * 1024;
const LOG_KEEP: usize = 200 * 1024;

/// `%USERPROFILE%\.claude\claudibeat\claudibeat.log`. Not under AppData: a
/// Claude chat reading AppData sees the Claude package's own copy of it, so a
/// log there would look empty to the agents asked why claudibeat fell.
pub fn log_path() -> PathBuf {
    // A test points the log elsewhere.
    let base = std::env::var_os("CLAUDIBEAT_LOG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::win::shell::profile_dir().join(".claude"));
    base.join("claudibeat").join("claudibeat.log")
}

/// One line in the log, with the local time and the process id.
pub fn log(line: &str) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    trim(&path);
    let stamp = crate::text::local_stamp();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{stamp} [{}] {line}", std::process::id());
    }
}

fn trim(path: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(path) else { return };
    if meta.len() <= LOG_MAX {
        return;
    }
    let Ok(text) = std::fs::read(path) else { return };
    let mut from = text.len().saturating_sub(LOG_KEEP);
    // Start on a whole line.
    while from < text.len() && text[from] != b'\n' {
        from += 1;
    }
    let _ = std::fs::write(path, &text[(from + 1).min(text.len())..]);
}

/// Write every panic to the log before the abort ends the process.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map_or_else(String::new, |l| format!(" at {}:{}", l.file(), l.line()));
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        log(&format!("PANIC{place}: {what}"));
    }));
}

/// Run the strip as a child and bring it back when it dies. Returns when the
/// child quits as asked, when another guard already runs, or when it keeps dying.
pub fn supervise() {
    let Some(_held) = guard_mutex() else { return };
    let Ok(exe) = std::env::current_exe() else {
        log("guard: no path to itself, running unguarded");
        return;
    };
    let mut deaths: Vec<Instant> = Vec::new();
    loop {
        let started = Instant::now();
        let status = std::process::Command::new(&exe).arg(CHILD_FLAG).status();
        let code = match status {
            Ok(s) => s.code(),
            Err(e) => {
                log(&format!("guard: could not start the strip: {e}"));
                return;
            }
        };
        if code == Some(0) {
            return; // a quit that was asked for, or another strip already runs
        }
        let lived = started.elapsed().as_secs();
        log(&format!("guard: the strip ended with code {code:?} after {lived} s; starting it again"));
        let now = Instant::now();
        deaths.retain(|t| now.duration_since(*t) < WINDOW);
        deaths.push(now);
        if deaths.len() > MAX_RESTARTS {
            log(&format!("guard: {} deaths in 10 min, giving up until the next sign-in", deaths.len()));
            return;
        }
        std::thread::sleep(PAUSE);
    }
}

const GUARD_NAME: windows::core::PCWSTR = windows::core::w!("Local\\claudibeat.guard");

/// `Local\claudibeat.guard` for the guard's life; `None` when a guard runs.
fn guard_mutex() -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    // SAFETY: creates or opens a named mutex; the handle lives until exit.
    unsafe {
        let h = CreateMutexW(None, true, GUARD_NAME).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(h)
    }
}

/// Whether a guard is running now.
pub fn guard_running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    // SAFETY: opens and at once closes a named mutex, only to see that it exists.
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, GUARD_NAME) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}
