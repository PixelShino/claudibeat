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
        let lang = match args.iter().position(|a| a == "--lang").and_then(|i| args.get(i + 1)).map(String::as_str) {
            Some("ru") => claudibeat::text::Lang::Ru,
            _ => claudibeat::text::Lang::En,
        };
        let code = claudibeat::cli::render_samples(&dir, value("--bg").as_deref(), lang).map_or(1, |()| 0);
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--quit") {
        std::process::exit(if claudibeat::app::quit_running(10_000) { 0 } else { 1 });
    }
    if let Some(file) = value("--dump") {
        std::process::exit(claudibeat::cli::dump(&file).map_or(1, |()| 0));
    }
    // The checks above may run beside a live claudibeat; a second live one may not.
    let Some(_instance) = claudibeat::app::single_instance() else { return };
    claudibeat::app::run();
}
