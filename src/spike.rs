//! Throwaway probe (`claudibeat --spike`): put the live strip on the real
//! taskbar and see that the layered child window, transparency and text hold
//! up. Right-click quits. Replaced by `app` in Task 9.

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
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn log(line: String) {
    use std::io::Write;
    let path = std::env::temp_dir().join("claudibeat-spike.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn redraw(hwnd: HWND) {
    let Some(bar) = taskbar::primary() else { return };
    // Render with the state borrowed, but touch the window only after the
    // borrow ends: SetWindowPos re-enters the window procedure synchronously.
    let frame = STATE.with_borrow_mut(|s| {
        let s = s.as_mut()?;
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
        s.renderer
            .render(width, h_dip.max(STRIP_H), scale, TRANSPARENT, |p| {
                let l = layout_strip(&model, p);
                paint_strip(p, &l, &model, None);
            })
            .ok()
    });
    if let Some(frame) = frame {
        taskbar::place_left(hwnd, bar, frame.width as i32);
        let r = layered::present(hwnd, &frame, None);
        // SAFETY: a query on our own window.
        let parent = unsafe { GetParent(hwnd) };
        log(format!("redraw {}x{} present={r:?} parent={parent:?} bar={bar:?}", frame.width, frame.height));
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let created = CREATED.get();
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

pub fn run() {
    // SAFETY: plain Win32 start-up and message loop.
    unsafe {
        let Ok(renderer) = Renderer::new() else { return };
        STATE.set(Some(State { renderer, store: Store::new(Store::default_dir()) }));
        CREATED.set(taskbar::created_msg());
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
            log(format!("create failed {:?}", windows::core::Error::from_win32()));
            return;
        };
        log(format!("created {hwnd:?}"));
        let Some(bar) = taskbar::primary() else { log("no taskbar".into()); return };
        taskbar::embed(hwnd, bar);
        log(format!("embedded parent={:?}", GetParent(hwnd)));
        redraw(hwnd);
        SetTimer(hwnd, 1, 2_000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
