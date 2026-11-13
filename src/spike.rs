//! Throwaway probe (`claudibeat --spike`): put the live strip on the real
//! taskbar and see that the layered child window, transparency and text hold
//! up, including across an Explorer restart. Right-click quits. Replaced by
//! `app` in Task 9.
//!
//! A hidden top-level host window owns the timer and hears `TaskbarCreated`
//! (broadcast to top-level windows only). The strip is a child of the
//! taskbar, so Explorer takes it down with the taskbar; the host simply
//! creates a new one.

use claudibeat::limits::{limit_views, strip_limit};
use claudibeat::merge::{counts, merge_rows};
use claudibeat::render::strip::{STRIP_H, StripModel, layout_strip, paint_strip};
use claudibeat::render::{Renderer, TRANSPARENT};
use claudibeat::store::Store;
use claudibeat::win::{layered, taskbar};
use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

const HOST_CLASS: windows::core::PCWSTR = w!("claudibeat.host");
const STRIP_CLASS: windows::core::PCWSTR = w!("claudibeat.strip");

struct State {
    renderer: Renderer,
    store: Store,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CREATED: Cell<u32> = const { Cell::new(0) };
    static STRIP: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
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

/// The strip window, created and embedded if the taskbar has none of ours.
fn ensure_strip() -> Option<(HWND, HWND)> {
    let bar = taskbar::primary()?;
    let current = STRIP.get();
    // SAFETY: queries and window creation on this thread.
    unsafe {
        if !current.is_invalid() && IsWindow(current).as_bool() && GetParent(current).ok() == Some(bar) {
            return Some((current, bar));
        }
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            STRIP_CLASS,
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
        )
        .ok()?;
        taskbar::embed(hwnd, bar);
        STRIP.set(hwnd);
        log(format!("strip created {hwnd:?} parent={:?}", GetParent(hwnd)));
        Some((hwnd, bar))
    }
}

fn redraw() {
    let Some((hwnd, bar)) = ensure_strip() else { return };
    // Render with the state borrowed, but touch the window only after the
    // borrow ends: SetWindowPos re-enters window procedures synchronously.
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
            limit: strip_limit(&views).map(|v| (v.used, v.used_fraction())),
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
        if let Err(e) = layered::present(hwnd, &frame, None) {
            log(format!("present failed {e:?}"));
        }
    }
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let created = CREATED.get();
    // SAFETY: standard window procedure calls.
    unsafe {
        match msg {
            WM_TIMER => {
                redraw();
                LRESULT(0)
            }
            m if m == created && created != 0 => {
                log("TaskbarCreated".into());
                STRIP.set(HWND(std::ptr::null_mut()));
                redraw();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: standard window procedure calls.
    unsafe {
        match msg {
            WM_RBUTTONUP => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_DESTROY => {
                if STRIP.get() == hwnd {
                    STRIP.set(HWND(std::ptr::null_mut()));
                }
                log("strip destroyed".into());
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
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let procs: [(windows::core::PCWSTR, WNDPROC); 2] = [(HOST_CLASS, Some(host_proc)), (STRIP_CLASS, Some(strip_proc))];
        for (class, proc_) in procs {
            let wc = WNDCLASSW { lpfnWndProc: proc_, hInstance: instance.into(), lpszClassName: class, hCursor: cursor, ..Default::default() };
            RegisterClassW(&wc);
        }
        // A hidden top-level window: it never shows, but it hears broadcasts.
        let Ok(host) = CreateWindowExW(WS_EX_TOOLWINDOW, HOST_CLASS, w!("claudibeat host"), WS_POPUP, 0, 0, 0, 0, None, None, instance, None) else {
            return;
        };
        redraw();
        SetTimer(host, 1, 2_000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let strip = STRIP.get();
        if !strip.is_invalid() {
            let _ = DestroyWindow(strip);
        }
        let _ = DestroyWindow(host);
    }
}
