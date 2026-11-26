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
        let theme = match args.iter().position(|a| a == "--theme").and_then(|i| args.get(i + 1)).map(String::as_str) {
            Some("dark") => claudibeat::render::Theme::Dark,
            _ => claudibeat::render::Theme::Light,
        };
        let code = claudibeat::cli::render_samples(&dir, value("--bg").as_deref(), lang, theme).map_or(1, |()| 0);
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--quit") {
        std::process::exit(if claudibeat::app::quit_running(10_000) { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--setup") {
        let said = claudibeat::setup::run();
        for line in &said {
            claudibeat::guard::log(&format!("setup: {line}"));
        }
        // Start the strip, unless one already runs; started plainly it guards itself.
        if !claudibeat::guard::guard_running()
            && let Ok(exe) = std::env::current_exe()
        {
            let _ = std::process::Command::new(exe).spawn();
        }
        if !args.iter().any(|a| a == "--quiet") {
            let text = format!("{}\n\nclaudibeat is in the left corner of the taskbar. Open a new Claude chat so the mod starts reporting.", said.join("\n"));
            // SAFETY: a modal message box with no owner.
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    None,
                    &windows::core::HSTRING::from(text),
                    windows::core::w!("claudibeat setup"),
                    windows::Win32::UI::WindowsAndMessaging::MB_OK | windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
                );
            }
        }
        return;
    }
    if let Some(file) = value("--dump") {
        std::process::exit(claudibeat::cli::dump(&file).map_or(1, |()| 0));
    }
    claudibeat::guard::install_panic_hook();
    // Started plainly (sign-in, a double click), claudibeat is a guard that
    // runs the strip as its child and starts it again if it dies.
    if !args.iter().any(|a| a == claudibeat::guard::CHILD_FLAG) {
        // Started from inside an app package (a Claude chat's command), it
        // would die with that app: hand over to the sign-in task instead.
        if let Some(package) = claudibeat::win::shell::started_under_claude().or_else(claudibeat::win::shell::package_name)
            && claudibeat::win::shell::autostart_get()
            && claudibeat::win::shell::run_task()
        {
            claudibeat::guard::log(&format!("guard: started inside {package}; handed over to the sign-in task"));
            return;
        }
        claudibeat::guard::supervise();
        return;
    }
    // The checks above may run beside a live claudibeat; a second live one may not.
    let Some(_instance) = claudibeat::app::single_instance() else { return };
    claudibeat::guard::log(&format!("strip: started ({})", claudibeat::win::shell::folder_sources()));
    claudibeat::app::run();
    claudibeat::guard::log("strip: quit as asked");
}
