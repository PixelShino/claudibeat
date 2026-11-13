//! Throwaway probe: put the live strip on the real taskbar and see that the
//! layered child window, transparency and text hold up. Right-click quits.
#![windows_subsystem = "windows"]

use claudibeat::limits::{limit_views, strip_limit};
use claudibeat::merge::{counts, merge_rows};
use claudibeat::render::strip::{STRIP_H, StripModel, layout_strip, paint_strip};
use claudibeat::render::{Renderer, TRANSPARENT};
use claudibeat::store::Store;
use claudibeat::win::{layered, taskbar};
use std::cell::RefCell;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

struct State {
    renderer: Renderer,
    store: Store,
    created: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn redraw(hwnd: HWND) {
    let Some(bar) = taskbar::primary() else { return };
    STATE.with_borrow_mut(|s| {
        let Some(s) = s else { return };
        let snap = s.store.read();
        let now = now_ms();
        let c = counts(&merge_rows(&snap.index, &snap.files, &snap.acks, now));
        let mem = claudibeat::memory::sample();
        let views = limit_views(snap.account.as_ref(), now);
        let model = StripModel {
            waiting: c.waiting,
            done: c.done,
            working: c.working,
            mem_percent: mem.percent(),
            mem_alert: mem.alert(),
            limit: strip_limit(&views).map(|v| (v.remaining, v.left_fraction())),
        };
        let scale = taskbar::scale(bar);
        let h_dip = taskbar::height(bar) as f32 / scale;
        let mut width = 0.0;
        let _ = s.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| width = layout_strip(&model, p).width);
        let Ok(frame) = s.renderer.render(width, h_dip.max(STRIP_H), scale, TRANSPARENT, |p| {
            let l = layout_strip(&model, p);
            paint_strip(p, &l, &model, None);
        }) else {
            return;
        };
        taskbar::place_left(hwnd, bar, frame.width as i32);
        let _ = layered::present(hwnd, &frame, None);
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let created = STATE.with_borrow(|s| s.as_ref().map_or(0, |s| s.created));
    // SAFETY: standard window procedure calls on our own window.
    unsafe {
        match msg {
            WM_TIMER => {
                redraw(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            m if m == created && created != 0 => {
                if let Some(bar) = taskbar::primary() {
                    taskbar::embed(hwnd, bar);
                }
                redraw(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn main() {
    // SAFETY: plain Win32 start-up and message loop.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(renderer) = Renderer::new() else { return };
        STATE.set(Some(State { renderer, store: Store::new(Store::default_dir()), created: taskbar::created_msg() }));
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: w!("claudibeat.spike"), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("claudibeat.spike"),
            w!("claudibeat"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            instance,
            None,
        ) else {
            return;
        };
        let Some(bar) = taskbar::primary() else { return };
        taskbar::embed(hwnd, bar);
        redraw(hwnd);
        SetTimer(hwnd, 1, 2_000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
