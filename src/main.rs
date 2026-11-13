#![windows_subsystem = "windows"]

use std::path::PathBuf;

fn main() {
    // SAFETY: called first, before any window exists.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    if let Some(dir) = value("--render") {
        let code = claudibeat::cli::render_samples(&dir, value("--bg").as_deref()).map_or(1, |()| 0);
        std::process::exit(code);
    }
}
