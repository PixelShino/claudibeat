//! The running program: one thread, one message loop.
//!
//! A hidden top-level host window owns the timers and hears `TaskbarCreated`
//! (Explorer broadcasts it to top-level windows only). The strip is a child
//! of the taskbar and dies with it; the host simply builds a new one. Window
//! procedures queue [`Input`]s; the host hands them to [`App`] with its state
//! borrowed only here, never inside another window procedure.

use crate::board::{Assignment, BoardFile, Column, Mailbox, Pending, REQUEST_TIMEOUT_MS, Request, Target, project_folders, request_id};
use crate::limits::{LimitView, limit_views, strip_limit};
use crate::memory::{self, MemGroups};
use crate::merge::{Row, counts, drop_helpers, groups, merge_rows, open_target};
use crate::notify::{Notifier, ToastKind, ToastStack};
use crate::render::board::{self as board_ui, BoardHit, BoardLayout, BoardModel, Drag, Drop, Status, Tone, layout_board, paint_board};
use crate::render::flyout::{FlyoutHit, FlyoutLayout, FlyoutModel, FlyoutView, M_BOTTOM, M_SIDE, Prefs, layout_flyout, paint_flyout};
use crate::render::strip::{STRIP_H, StripItem, StripLayout, StripModel, layout_strip, paint_strip};
use crate::render::toast::{T_SIDE, T_TOP, TOAST_W, ToastModel, layout_toast, paint_toast, paint_tooltip, toast_model, tooltip_size};
use crate::render::{Rect, Renderer, TRANSPARENT};
use crate::settings::Settings;
use crate::store::Store;
use crate::text::{local_tz_offset_min, tip_done, tip_limit, tip_memory, tip_tasks, tip_waiting, tip_working};
use crate::win::popup::{self, Popup};
use crate::win::strip_window::{self, StripWindow};
use crate::win::{Action, Input, Surface, WM_FOLDER_CHANGED, WM_INPUT_READY, cursor_over, set_hand, shell, taskbar};
use std::cell::{Cell, RefCell};
use crate::model::SessionState;
use std::collections::HashMap;
use std::path::PathBuf;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const HOST_CLASS: PCWSTR = w!("claudibeat.host");

const T_REFRESH: usize = 1;
const T_MEMORY: usize = 2;
const T_TRAY: usize = 3;
/// Fires shortly after the status-band folder changes, to read a burst once.
const T_SOON: usize = 4;
const T_TIP: usize = 5;
const T_TOAST: usize = 6;
/// Ticks while the board waits on requests, to read replies quickly.
const T_BOARD: usize = 7;
const BOARD_TICK_MS: u32 = 400;
/// The board asks the mod for fresh data when what it has is older than this.
const BOARD_STALE_MS: i64 = 60_000;
/// A line at the board's foot stays this long.
const STATUS_MS: i64 = 9_000;
/// A press becomes a drag once the pointer travels this far, in DIPs.
const DRAG_SLOP: f32 = 5.0;

const REFRESH_MS: u32 = 2_000;
const MEMORY_MS: u32 = 3_000;
const TRAY_MS: u32 = 10_000;
const SOON_MS: u32 = 150;
const TIP_MS: u32 = 600;
const TOAST_TICK_MS: u32 = 250;
/// A «готово» toast leaves after this long on screen, not counting hover.
const DONE_SHOWN_MS: i64 = 8_000;

/// Gap between the taskbar's top edge and a card above it, in DIPs.
const GAP: f32 = 12.0;
/// Cards line up with the strip's first item.
const EDGE: f32 = 8.0;
const TOAST_GAP: f32 = 8.0;
const SCROLL_STEP: f32 = 48.0;
/// The strip's items take clicks 4 DIPs above and below their 32 DIPs.
const HIT_SLOP: f32 = 4.0;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static CREATED: Cell<u32> = const { Cell::new(0) };
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn shifted(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.w, r.h)
}

/// Where the taskbar is, in physical pixels.
struct Geo {
    rect: RECT,
    scale: f32,
    work_top: i32,
}

fn geometry() -> Option<Geo> {
    let bar = taskbar::primary()?;
    let mut rect = RECT::default();
    // SAFETY: queries with local out parameters.
    unsafe {
        GetWindowRect(bar, &mut rect).ok()?;
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let mut work_top = 0;
        if GetMonitorInfoW(MonitorFromWindow(bar, MONITOR_DEFAULTTOPRIMARY), &mut mi).as_bool() {
            work_top = mi.rcWork.top;
            // An auto-hidden taskbar slides below the screen; cards stay
            // where they sit when it is shown, so nothing jumps or overlaps.
            let h = rect.bottom - rect.top;
            if rect.top > mi.rcMonitor.bottom - h {
                rect.top = mi.rcMonitor.bottom - h;
                rect.bottom = mi.rcMonitor.bottom;
            }
        }
        Some(Geo { rect, scale: taskbar::scale(bar), work_top })
    }
}

#[derive(PartialEq)]
struct StripKey {
    hwnd: isize,
    model: StripModel,
    hover: Option<StripItem>,
    height: i32,
    scale: u32,
}

struct StripGeo {
    layout: StripLayout,
    scale: f32,
    dy: f32,
}

struct FlyGeo {
    layout: FlyoutLayout,
    dx: f32,
    scale: f32,
}

struct BoardGeo {
    layout: BoardLayout,
    dx: f32,
    scale: f32,
}

/// The left button is down on the board: maybe a click, maybe a drag.
struct Press {
    hit: Option<BoardHit>,
    /// Board DIPs where it went down.
    x: f32,
    y: f32,
    /// The pressed card's corner relative to the pointer.
    grab: (f32, f32),
}

struct ToastHit {
    index: usize,
    card: Rect,
    close: Rect,
}

/// What a toast frame shows; an identical one is not painted again.
#[derive(PartialEq)]
struct ToastKey {
    models: Vec<ToastModel>,
    hover_close: Option<usize>,
    left: i32,
    top: i32,
    scale: u32,
}

struct ToastGeo {
    hits: Vec<ToastHit>,
    scale: f32,
}

pub struct App {
    instance: HINSTANCE,
    host: HWND,
    renderer: Renderer,
    store: Store,
    settings: Settings,
    settings_path: PathBuf,
    tz: i32,
    rows: Vec<Row>,
    limits: Vec<LimitView>,
    mem: MemGroups,
    notifier: Notifier,
    stack: ToastStack,
    /// On-screen time of each «готово» toast, by its `at`.
    done_shown: HashMap<i64, i64>,
    busy: bool,

    strip: StripWindow,
    strip_key: Option<StripKey>,
    strip_geo: Option<StripGeo>,
    hover: Option<StripItem>,

    tip: Popup,
    tip_item: Option<StripItem>,

    flyout: Popup,
    fly_geo: Option<FlyGeo>,
    expanded: [bool; 3],
    scroll: f32,
    fly_hover: Option<FlyoutHit>,
    fly_view: FlyoutView,

    toasts: Popup,
    toast_geo: Option<ToastGeo>,
    toast_hovered: bool,
    toast_hover_close: Option<usize>,
    toast_timer: bool,
    toast_key: Option<ToastKey>,
    /// Whether the logon task exists; asking means running `schtasks`.
    autostart: bool,

    board_file: Option<BoardFile>,
    assignments: Vec<Assignment>,
    /// Holders learnt from replies, kept while the card is in progress.
    seen_holders: Vec<Assignment>,
    mailbox: Mailbox,
    board: Popup,
    board_geo: Option<BoardGeo>,
    board_scroll: [f32; 4],
    board_hover: Option<BoardHit>,
    board_filter: Option<String>,
    press: Option<Press>,
    drag: Option<Drag>,
    pending: Vec<Pending>,
    status: Option<(Status, i64)>,
    salt: u32,
    /// Moves arrive faster than frames: the board is drawn once per batch.
    board_dirty: bool,
}

impl App {
    fn new(instance: HINSTANCE, host: HWND) -> Option<App> {
        let settings_path = Settings::default_path();
        shell::migrate_autostart();
        Some(App {
            instance,
            host,
            renderer: Renderer::new().ok()?,
            store: Store::new(Store::default_dir()),
            settings: Settings::load(&settings_path),
            settings_path,
            tz: local_tz_offset_min(),
            rows: Vec::new(),
            limits: Vec::new(),
            mem: memory::sample(),
            notifier: Notifier::new(),
            stack: ToastStack::default(),
            done_shown: HashMap::new(),
            busy: false,
            strip: StripWindow::default(),
            strip_key: None,
            strip_geo: None,
            hover: None,
            tip: Popup::new(instance, Surface::Tooltip)?,
            tip_item: None,
            flyout: Popup::new(instance, Surface::Flyout)?,
            fly_geo: None,
            expanded: [false; 3],
            scroll: 0.0,
            fly_hover: None,
            fly_view: FlyoutView::Sessions,
            toasts: Popup::new(instance, Surface::Toasts)?,
            toast_geo: None,
            toast_hovered: false,
            toast_hover_close: None,
            toast_timer: false,
            toast_key: None,
            autostart: shell::autostart_get(),
            board_file: None,
            assignments: Vec::new(),
            seen_holders: Vec::new(),
            mailbox: Mailbox::new(&Store::default_dir()),
            board: Popup::new(instance, Surface::Board)?,
            board_geo: None,
            board_scroll: [0.0; 4],
            board_hover: None,
            board_filter: None,
            press: None,
            drag: None,
            pending: Vec::new(),
            status: None,
            salt: 0,
            board_dirty: false,
        })
    }

    fn timer(&self, id: usize, ms: u32) {
        // SAFETY: a timer on our own window; re-setting an id restarts it.
        unsafe { SetTimer(self.host, id, ms, None) };
    }

    fn kill(&self, id: usize) {
        // SAFETY: as above.
        let _ = unsafe { KillTimer(self.host, id) };
    }

    // ---- data -------------------------------------------------------------

    /// Re-read the status-band folder, raise toasts for what changed, redraw.
    fn refresh(&mut self) {
        let snap = self.store.read();
        let now = now_ms();
        self.tz = local_tz_offset_min();
        self.rows = merge_rows(&snap.index, &snap.files, &snap.acks, now);
        if self.settings.quiet_helpers {
            drop_helpers(&mut self.rows);
        }
        self.limits = limit_views(snap.account.as_ref(), now);
        self.board_file = snap.board;
        if let Some(b) = &self.board_file {
            self.seen_holders.retain(|a| b.doing.iter().any(|c| c.key == a.task));
        }
        self.assignments = snap.assignments;
        // Until a leading mod writes them down, the replies this claudibeat
        // saw still say who holds what.
        let seen = &self.seen_holders;
        let missing: Vec<Assignment> = seen.iter().filter(|a| !self.assignments.iter().any(|b| b.task == a.task)).cloned().collect();
        self.assignments.extend(missing);
        self.read_replies(now);
        self.busy = shell::is_busy();
        // WM_SETTINGCHANGE brings a switch at once; this catches a missed one.
        self.sync_theme();

        let events = self.notifier.observe(&self.rows, now);
        let needing: HashMap<String, SessionState> = self.rows.iter().filter(|r| r.needs_you).map(|r| (r.key.clone(), r.state)).collect();
        self.stack.retain_needing(&needing);
        if self.settings.dnd {
            // Do not disturb: the notifier still learns what changed, so
            // nothing old pops up when it is switched off; nothing is shown.
        } else if let Some(first) = events.first() {
            let chime = if events.iter().any(|e| e.kind.is_waiting()) { ToastKind::Waiting } else { first.kind };
            for e in events {
                self.stack.push(e);
            }
            // In a full-screen game or presentation the toast waits, silently.
            if self.settings.sound && !self.busy {
                crate::sound::play(chime);
            }
        }
        self.forget_gone_toasts();
        self.draw_all();
    }

    fn forget_gone_toasts(&mut self) {
        let stack = &self.stack;
        self.done_shown.retain(|at, _| stack.items.iter().any(|e| e.at == *at && !e.kind.is_waiting()));
        if self.toast_hover_close.is_some_and(|i| i >= self.stack.items.len()) {
            self.toast_hover_close = None;
        }
    }

    /// Follow the taskbar's light or dark mode; on a change, every surface
    /// is drawn again. True when it changed.
    fn sync_theme(&mut self) -> bool {
        let theme = shell::system_theme();
        if theme == self.renderer.theme() {
            return false;
        }
        self.renderer.set_theme(theme);
        self.strip_key = None;
        self.toast_key = None;
        true
    }

    fn prefs(&self) -> Prefs {
        Prefs { dnd: self.settings.dnd, sound: self.settings.sound, autostart: self.autostart, quiet_helpers: self.settings.quiet_helpers }
    }

    fn strip_model(&self) -> StripModel {
        let c = counts(&self.rows);
        StripModel {
            waiting: c.waiting,
            done: c.done,
            working: c.working,
            tasks: self.board_file.as_ref().map(|b| b.column(Column::Doing).len()),
            mem_percent: self.mem.percent(),
            mem_alert: self.mem.alert(),
            limit: strip_limit(&self.limits).map(|v| (v.used, v.used_fraction())),
        }
    }

    fn tip_text(&self, item: StripItem) -> String {
        let c = counts(&self.rows);
        let lang = self.settings.language;
        match item {
            StripItem::Waiting => tip_waiting(lang, c.waiting),
            StripItem::Done => tip_done(lang, c.done),
            StripItem::Working => tip_working(lang, c.working),
            StripItem::Tasks => self.board_file.as_ref().map_or_else(|| lang.board().tip_tasks.to_owned(), |b| tip_tasks(lang, b.column(Column::Doing).len(), b.todo.len())),
            StripItem::Memory => tip_memory(lang, self.mem.used, self.mem.total),
            StripItem::Limit => strip_limit(&self.limits).map_or_else(|| lang.words().limit_unknown.to_owned(), |v| tip_limit(lang, v, now_ms(), self.tz)),
        }
    }

    // ---- drawing ----------------------------------------------------------

    /// Re-derive hover state from where the pointer really is, so a lost
    /// mouse-leave can never leave a highlight or a paused toast behind.
    fn sync_pointer(&mut self) {
        let over = if self.toasts.visible() { cursor_over(self.toasts.hwnd()) } else { None };
        let (hovered, close) = match over {
            Some((x, y)) => (true, self.toast_at(x, y).1),
            None => (false, None),
        };
        self.toast_hovered = hovered;
        self.toast_hover_close = close;
        if self.hover.is_some() && self.strip.hwnd().and_then(cursor_over).is_none() {
            self.hover = None;
            self.hide_tip();
        }
        if self.fly_hover.is_some() && !(self.flyout.visible() && cursor_over(self.flyout.hwnd()).is_some()) {
            self.fly_hover = None;
        }
        if self.board_hover.is_some() && self.drag.is_none() && !(self.board.visible() && cursor_over(self.board.hwnd()).is_some()) {
            self.board_hover = None;
        }
    }

    fn draw_all(&mut self) {
        self.sync_pointer();
        self.draw_strip();
        if self.flyout.visible() {
            self.draw_flyout();
        }
        if self.board.visible() {
            self.draw_board();
        }
        self.draw_toasts();
        if let Some(item) = self.tip_item {
            self.show_tip(item);
        }
    }

    fn draw_strip(&mut self) {
        let Some((hwnd, bar)) = self.strip.ensure(self.instance) else { return };
        let model = self.strip_model();
        let scale = taskbar::scale(bar);
        let height = taskbar::height(bar);
        let key = StripKey { hwnd: hwnd.0 as isize, model, hover: self.hover, height, scale: scale.to_bits() };
        if self.strip_key.as_ref() == Some(&key) {
            return;
        }
        let h_dip = (height as f32 / scale).max(1.0);
        let dy = ((h_dip - STRIP_H) / 2.0).round();
        let mut layout = None;
        let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| layout = Some(layout_strip(&model, p)));
        let Some(layout) = layout else { return };
        let hover = self.hover;
        let Ok(frame) = self.renderer.render(layout.width, h_dip, scale, TRANSPARENT, |p| p.translate(0.0, dy, |p| paint_strip(p, &layout, &model, hover))) else {
            return;
        };
        self.strip_key = self.strip.show(hwnd, bar, &frame).is_ok().then_some(key);
        self.strip_geo = Some(StripGeo { layout, scale, dy });
    }

    fn draw_flyout(&mut self) {
        let Some(geo) = geometry() else { return };
        let model = FlyoutModel {
            groups: groups(&self.rows),
            total: self.rows.len(),
            expanded: self.expanded,
            mem: self.mem,
            limits: self.limits.clone(),
            now: now_ms(),
            tz: self.tz,
            lang: self.settings.language,
            view: self.fly_view,
            prefs: self.prefs(),
        };
        // The window may reach the top of the work area; the list scrolls.
        let avail = (geo.rect.top - geo.work_top) as f32 / geo.scale;
        let max_h = avail + M_BOTTOM - GAP;
        let mut layout = None;
        let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| layout = Some(layout_flyout(&model, p, max_h)));
        let Some(l) = layout else { return };
        self.scroll = self.scroll.clamp(0.0, l.scroll_max);
        let dx = EDGE - l.card.x;
        let (w, h) = (EDGE + l.card.w + M_SIDE, l.card.bottom() + GAP);
        let (hover, scroll) = (self.fly_hover.as_ref(), self.scroll);
        let Ok(frame) = self.renderer.render(w, h, geo.scale, TRANSPARENT, |p| p.translate(dx, 0.0, |p| paint_flyout(p, &l, &model, hover, scroll))) else {
            return;
        };
        let _ = self.flyout.show(&frame, geo.rect.left, geo.rect.top - frame.height as i32);
        self.fly_geo = Some(FlyGeo { layout: l, dx, scale: geo.scale });
    }

    fn draw_toasts(&mut self) {
        let geo = geometry();
        let (Some(geo), false, false, false) = (geo, self.stack.items.is_empty(), self.flyout.visible() || self.board.visible(), self.busy) else {
            self.toasts.hide();
            self.toast_geo = None;
            self.toast_key = None;
            // A hidden window may never hear the pointer leave.
            self.toast_hovered = false;
            self.toast_hover_close = None;
            self.sync_toast_timer();
            return;
        };
        let now = now_ms();
        let lang = self.settings.language;
        let models: Vec<ToastModel> = self.stack.items.iter().map(|e| toast_model(e, &self.rows, now, lang)).collect();
        let mut layouts = Vec::new();
        let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| layouts = models.iter().map(|m| layout_toast(m, p)).collect());
        if layouts.len() != models.len() {
            return;
        }
        let key = ToastKey { models: models.clone(), hover_close: self.toast_hover_close, left: geo.rect.left, top: geo.rect.top, scale: geo.scale.to_bits() };
        if self.toasts.visible() && self.toast_key.as_ref() == Some(&key) {
            return;
        }
        // Oldest on top, newest closest to the strip.
        let mut tops = Vec::with_capacity(layouts.len());
        let mut y = T_TOP;
        for l in &layouts {
            tops.push(y);
            y += l.card.h + TOAST_GAP;
        }
        let (w, h) = (EDGE + TOAST_W + T_SIDE, y - TOAST_GAP + GAP);
        let dx = EDGE - T_SIDE;
        let hover_close = self.toast_hover_close;
        let paint = |p: &crate::render::Painter| {
            for (i, (m, l)) in models.iter().zip(&layouts).enumerate() {
                p.translate(dx, tops[i] - l.card.y, |p| paint_toast(p, l, m, hover_close == Some(i)));
            }
        };
        let Ok(frame) = self.renderer.render(w, h, geo.scale, TRANSPARENT, paint) else { return };
        let hits = layouts
            .iter()
            .enumerate()
            .map(|(index, l)| {
                let dy = tops[index] - l.card.y;
                ToastHit { index, card: shifted(l.card, dx, dy), close: shifted(l.close, dx, dy) }
            })
            .collect();
        let _ = self.toasts.show(&frame, geo.rect.left, geo.rect.top - frame.height as i32);
        self.toast_geo = Some(ToastGeo { hits, scale: geo.scale });
        self.toast_key = Some(key);
        self.sync_toast_timer();
    }

    /// The «готово» countdown ticks only while such a toast is on screen.
    fn sync_toast_timer(&mut self) {
        let want = self.toasts.visible() && self.stack.items.iter().any(|e| !e.kind.is_waiting());
        if want != self.toast_timer {
            self.toast_timer = want;
            if want {
                self.timer(T_TOAST, TOAST_TICK_MS);
            } else {
                self.kill(T_TOAST);
            }
        }
    }

    fn show_tip(&mut self, item: StripItem) {
        let (Some(geo), Some(sg)) = (geometry(), self.strip_geo.as_ref()) else { return };
        let Some(&(_, rect)) = sg.layout.items.iter().find(|(k, _)| *k == item) else { return };
        let text = self.tip_text(item);
        let mut size = (0.0, 0.0);
        let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| size = tooltip_size(p, &text));
        let Ok(frame) = self.renderer.render(size.0, size.1, geo.scale, TRANSPARENT, |p| paint_tooltip(p, &text)) else { return };
        // Centre the card (8 DIPs into the window, 16 narrower) on the item.
        let cx = geo.rect.left as f32 + (rect.x + rect.w / 2.0) * geo.scale;
        let x = ((cx - (8.0 + (size.0 - 16.0) / 2.0) * geo.scale).round() as i32).max(geo.rect.left);
        let y = geo.rect.top + (2.0 * geo.scale).round() as i32 - frame.height as i32;
        let _ = self.tip.show(&frame, x, y);
        self.tip_item = Some(item);
    }

    fn hide_tip(&mut self) {
        self.kill(T_TIP);
        self.tip.hide();
        self.tip_item = None;
    }

    // ---- actions ----------------------------------------------------------

    fn act(&mut self, action: Action) {
        match action {
            Action::OpenChat(key) => self.open_chat(&key),
            Action::OpenFlyout if !self.flyout.visible() => self.open_flyout(),
            Action::OpenFlyout => {}
            Action::ToggleFlyout if self.flyout.visible() => self.close_flyout(),
            Action::ToggleFlyout => self.open_flyout(),
            Action::ToggleBoard if self.board.visible() => self.close_board(),
            Action::ToggleBoard => self.open_board(),
            Action::MarkAllSeen => {
                let now = now_ms();
                for r in self.rows.iter().filter(|r| r.needs_you) {
                    let _ = self.store.write_ack(&r.key, now);
                }
                self.refresh();
            }
            Action::ToggleSound => {
                self.settings.sound = !self.settings.sound;
                let _ = self.settings.save(&self.settings_path);
                self.redraw_flyout();
            }
            Action::ToggleAutostart => {
                shell::autostart_set(!self.autostart);
                self.autostart = shell::autostart_get();
                self.redraw_flyout();
            }
            Action::ToggleHelpers => {
                self.settings.quiet_helpers = !self.settings.quiet_helpers;
                let _ = self.settings.save(&self.settings_path);
                let _ = self.store.write_tray(now_ms(), self.settings.quiet_helpers);
                self.refresh();
                self.draw_all();
            }
            Action::ToggleDnd => {
                self.settings.dnd = !self.settings.dnd;
                let _ = self.settings.save(&self.settings_path);
                if self.settings.dnd {
                    // Switching it on clears what is already on screen.
                    self.stack.items.clear();
                    self.forget_gone_toasts();
                }
                self.draw_all();
            }
            Action::SetLanguage(lang) => {
                self.settings.language = lang;
                let _ = self.settings.save(&self.settings_path);
                self.draw_all();
            }
            Action::Quit => {
                crate::guard::log("strip: Quit from the menu");
                // SAFETY: ends our own message loop.
                unsafe { PostQuitMessage(0) }
            }
        }
    }

    /// Open the chat, mark it seen (shared with the strip inside Claude),
    /// and let its toasts go.
    fn open_chat(&mut self, key: &str) {
        let Some(url) = self.rows.iter().find(|r| r.key == key).and_then(open_target).map(str::to_owned) else { return };
        if self.flyout.visible() {
            self.close_flyout();
        }
        if !shell::open_url(&url) {
            return; // nothing opened, so nothing is marked seen
        }
        let now = now_ms();
        let _ = self.store.write_ack(key, now);
        self.notifier.opened(key, now);
        self.refresh();
    }

    fn redraw_flyout(&mut self) {
        if self.flyout.visible() {
            self.draw_flyout();
        }
    }

    /// Switch the open window between the sessions and the settings.
    fn show_view(&mut self, view: FlyoutView) {
        self.fly_view = view;
        self.scroll = 0.0;
        self.fly_hover = None;
        self.draw_flyout();
    }

    fn open_flyout(&mut self) {
        self.close_board();
        self.hide_tip();
        self.expanded = [false; 3];
        self.scroll = 0.0;
        self.fly_hover = None;
        self.fly_view = FlyoutView::Sessions;
        self.draw_flyout();
        self.draw_toasts();
    }

    fn close_flyout(&mut self) {
        self.flyout.hide();
        self.fly_geo = None;
        self.fly_hover = None;
        self.draw_toasts();
    }

    // ---- input ------------------------------------------------------------

    fn on_input(&mut self, input: Input) {
        match input {
            Input::Move(Surface::Strip, x, y) => {
                let item = self.strip_item_at(x, y);
                if item != self.hover {
                    self.hover = item;
                    self.hide_tip();
                    if item.is_some() {
                        self.timer(T_TIP, TIP_MS);
                    }
                    self.draw_strip();
                }
            }
            Input::Leave(Surface::Strip) => {
                self.hover = None;
                self.hide_tip();
                self.draw_strip();
            }
            Input::Click(Surface::Strip, x, y) => {
                self.hide_tip();
                // Between and around the items, a click opens the list too.
                let action = self.strip_item_at(x, y).map_or(Action::ToggleFlyout, |item| self.strip_action(item));
                self.act(action);
            }
            Input::RightClick(Surface::Strip) => {
                self.hide_tip();
                let pending = self.rows.iter().any(|r| r.needs_you);
                if let Some(a) = strip_window::menu(self.host, pending, self.board_file.is_some(), self.prefs(), self.settings.language) {
                    self.act(a);
                }
            }

            Input::Move(Surface::Flyout, x, y) => {
                let hover = self.flyout_at(x, y).0.filter(|h| !matches!(h, FlyoutHit::More(_)));
                if hover != self.fly_hover {
                    self.fly_hover = hover;
                    self.draw_flyout();
                }
            }
            Input::Leave(Surface::Flyout) if self.fly_hover.is_some() => {
                self.fly_hover = None;
                self.draw_flyout();
            }
            Input::Click(Surface::Flyout, x, y) => match self.flyout_at(x, y) {
                (Some(FlyoutHit::Row(key)), _) => self.open_chat(&key),
                (Some(FlyoutHit::More(g)), _) => {
                    self.expanded[g] = true;
                    self.draw_flyout();
                }
                (Some(FlyoutHit::Gear), _) => self.show_view(FlyoutView::Settings),
                (Some(FlyoutHit::SeenAll), _) => self.act(Action::MarkAllSeen),
                (Some(FlyoutHit::Back), _) => self.show_view(FlyoutView::Sessions),
                (Some(FlyoutHit::Dnd), _) => self.act(Action::ToggleDnd),
                (Some(FlyoutHit::Sound), _) => self.act(Action::ToggleSound),
                (Some(FlyoutHit::Helpers), _) => self.act(Action::ToggleHelpers),
                (Some(FlyoutHit::Autostart), _) => self.act(Action::ToggleAutostart),
                (Some(FlyoutHit::Lang(lang)), _) => self.act(Action::SetLanguage(lang)),
                (None, false) => self.close_flyout(),
                (None, true) => {}
            },
            Input::Wheel(Surface::Flyout, delta) if self.flyout.visible() => {
                let max = self.fly_geo.as_ref().map_or(0.0, |g| g.layout.scroll_max);
                let next = (self.scroll - delta as f32 / 120.0 * SCROLL_STEP).clamp(0.0, max);
                if next != self.scroll {
                    self.scroll = next;
                    self.fly_hover = None;
                    self.draw_flyout();
                }
            }
            // Esc on the settings page steps back; on the list it closes.
            Input::Escape(Surface::Flyout) if self.flyout.visible() && self.fly_view == FlyoutView::Settings => self.show_view(FlyoutView::Sessions),
            Input::Escape(Surface::Flyout) | Input::Deactivated(Surface::Flyout) if self.flyout.visible() => self.close_flyout(),

            Input::Move(Surface::Board, x, y) => self.board_move(x, y),
            Input::Leave(Surface::Board) if self.board_hover.is_some() && self.drag.is_none() => {
                self.board_hover = None;
                self.board_dirty = true;
            }
            Input::Down(Surface::Board, x, y) => self.board_down(x, y),
            Input::Up(Surface::Board, x, y) => self.board_up(x, y),
            Input::Lost(Surface::Board) => {
                self.press = None;
                if self.drag.take().is_some() {
                    self.board_dirty = true;
                }
            }
            Input::Wheel(Surface::Board, delta) if self.board.visible() => self.board_wheel(delta),
            Input::Escape(Surface::Board) if self.drag.is_some() => {
                self.drag = None;
                self.press = None;
                self.board_dirty = true;
            }
            // Losing focus mid-drag (a toast, a dialog) keeps the board.
            Input::Deactivated(Surface::Board) if self.drag.is_some() => {}
            Input::Escape(Surface::Board) | Input::Deactivated(Surface::Board) if self.board.visible() => self.close_board(),

            Input::Move(Surface::Toasts, x, y) => {
                self.toast_hovered = true;
                let (on_card, close) = self.toast_at(x, y);
                set_hand(Surface::Toasts, on_card.is_some() && close.is_none());
                if close != self.toast_hover_close {
                    self.toast_hover_close = close;
                    self.draw_toasts();
                }
            }
            Input::Leave(Surface::Toasts) => {
                self.toast_hovered = false;
                if self.toast_hover_close.take().is_some() {
                    self.draw_toasts();
                }
            }
            Input::Click(Surface::Toasts, x, y) => match self.toast_at(x, y) {
                (_, Some(i)) => {
                    // The cross only closes: no mark, the counter stays.
                    self.stack.remove_at(i);
                    self.toast_hover_close = None;
                    self.forget_gone_toasts();
                    self.draw_toasts();
                }
                (Some(i), None) => {
                    let action = self.toast_action(i);
                    self.act(action);
                }
                (None, None) => {}
            },
            _ => {}
        }
    }

    fn strip_item_at(&self, x: i32, y: i32) -> Option<StripItem> {
        let g = self.strip_geo.as_ref()?;
        let (x, y) = (x as f32 / g.scale, y as f32 / g.scale - g.dy);
        g.layout.items.iter().find(|(_, r)| x >= r.x && x < r.right() && y >= r.y - HIT_SLOP && y < r.bottom() + HIT_SLOP).map(|(k, _)| *k)
    }

    /// One waiting or finished session opens straight away; otherwise the list.
    fn strip_action(&self, item: StripItem) -> Action {
        if item == StripItem::Tasks {
            return Action::ToggleBoard;
        }
        let g = groups(&self.rows);
        let only = match item {
            StripItem::Waiting if g.waiting.len() == 1 => g.waiting.first(),
            StripItem::Done if g.done.len() == 1 => g.done.first(),
            _ => None,
        };
        match only.filter(|r| open_target(r).is_some()) {
            Some(r) => Action::OpenChat(r.key.clone()),
            None => Action::ToggleFlyout,
        }
    }

    /// What is under the pointer, and whether it is on the card at all.
    fn flyout_at(&self, x: i32, y: i32) -> (Option<FlyoutHit>, bool) {
        let Some(g) = self.fly_geo.as_ref() else { return (None, false) };
        let (x, y) = (x as f32 / g.scale - g.dx, y as f32 / g.scale);
        (g.layout.hit(x, y, self.scroll), g.layout.card.contains(x, y))
    }

    /// The toast under the pointer, and the toast whose cross is under it.
    fn toast_at(&self, x: i32, y: i32) -> (Option<usize>, Option<usize>) {
        let Some(g) = self.toast_geo.as_ref() else { return (None, None) };
        let (x, y) = (x as f32 / g.scale, y as f32 / g.scale);
        let card = g.hits.iter().find(|h| h.card.contains(x, y)).map(|h| h.index);
        let close = g.hits.iter().find(|h| h.close.contains(x, y)).map(|h| h.index);
        (card, close)
    }

    /// A toast about one session opens its chat when it can; a summary, or a
    /// session without a link, opens the list instead.
    fn toast_action(&self, i: usize) -> Action {
        let Some(ev) = self.stack.items.get(i) else { return Action::OpenFlyout };
        let single = matches!(ev.kind, ToastKind::Waiting | ToastKind::Done);
        let key = ev.keys.first().filter(|_| single);
        match key.and_then(|k| self.rows.iter().find(|r| &r.key == k)).filter(|r| open_target(r).is_some()) {
            Some(r) => Action::OpenChat(r.key.clone()),
            None => Action::OpenFlyout,
        }
    }

    // ---- timers -----------------------------------------------------------

    fn on_timer(&mut self, id: usize) {
        match id {
            T_REFRESH => self.refresh(),
            T_SOON => {
                self.kill(T_SOON);
                self.refresh();
            }
            T_MEMORY => {
                self.mem = memory::sample();
                self.draw_strip();
                if self.flyout.visible() {
                    self.draw_flyout();
                }
                if self.tip_item == Some(StripItem::Memory) {
                    self.show_tip(StripItem::Memory);
                }
            }
            T_TRAY => {
                let _ = self.store.write_tray(now_ms(), self.settings.quiet_helpers);
            }
            T_TIP => {
                self.kill(T_TIP);
                if let (Some(item), false, false) = (self.hover, self.flyout.visible(), self.toasts.visible()) {
                    self.show_tip(item);
                }
            }
            T_TOAST => self.tick_toasts(),
            T_BOARD => {
                self.read_replies(now_ms());
                if self.pending.is_empty() {
                    self.kill(T_BOARD);
                }
                if self.board.visible() {
                    self.draw_board();
                }
            }
            _ => {}
        }
    }

    fn tick_toasts(&mut self) {
        let (close, hover) = (self.toast_hover_close, self.hover);
        self.sync_pointer();
        if close != self.toast_hover_close {
            self.draw_toasts();
        }
        if hover != self.hover {
            self.draw_strip();
        }
        if !self.toasts.visible() || self.toast_hovered {
            return;
        }
        for e in self.stack.items.iter().filter(|e| !e.kind.is_waiting()) {
            *self.done_shown.entry(e.at).or_insert(0) += TOAST_TICK_MS as i64;
        }
        let before = self.stack.items.len();
        let shown = &self.done_shown;
        self.stack.items.retain(|e| e.kind.is_waiting() || shown.get(&e.at).is_none_or(|&ms| ms < DONE_SHOWN_MS));
        if self.stack.items.len() != before {
            self.forget_gone_toasts();
            self.draw_toasts();
        }
    }

    // ---- the Plane board ----------------------------------------------------

    /// Chats a task can go to: who waits, who finished, who works, then the
    /// quiet ones, freshest first.
    fn board_agents(&self) -> Vec<&Row> {
        let rank = |r: &Row| match r.state {
            SessionState::Waiting if r.needs_you => 0,
            SessionState::Done if r.needs_you => 1,
            SessionState::Working => 2,
            _ => 3,
        };
        let mut rows: Vec<&Row> = self.rows.iter().collect();
        rows.sort_by_key(|r| (rank(r), std::cmp::Reverse(r.last_activity_at)));
        rows
    }

    fn board_folders(&self) -> Vec<(String, String)> {
        let seen: Vec<(String, String, i64)> = self.rows.iter().map(|r| (r.cwd.clone(), r.project.clone(), r.last_activity_at)).collect();
        project_folders(&seen, 6).into_iter().chain(std::iter::once((String::new(), String::new()))).collect()
    }

    fn board_status(&self, now: i64) -> Option<Status> {
        self.status.as_ref().filter(|(_, at)| now - at < STATUS_MS).map(|(s, _)| s.clone())
    }

    fn draw_board(&mut self) {
        let Some(geo) = geometry() else { return };
        let now = now_ms();
        let folders = self.board_folders();
        let model = BoardModel {
            board: self.board_file.as_ref(),
            agents: self.board_agents(),
            assignments: &self.assignments,
            pending: &self.pending,
            filter: self.board_filter.clone(),
            folders,
            now,
            lang: self.settings.language,
            status: self.board_status(now),
        };
        let avail = (geo.rect.top - geo.work_top) as f32 / geo.scale;
        let max_h = avail + board_ui::M_BOTTOM - GAP;
        let max_w = (geo.rect.right - geo.rect.left) as f32 / geo.scale - EDGE;
        let mut layout = None;
        let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| layout = Some(layout_board(&model, p, max_w, max_h)));
        let Some(l) = layout else { return };
        let mut scroll = self.board_scroll;
        for (s, max) in scroll.iter_mut().zip(l.scroll_max) {
            *s = s.clamp(0.0, max);
        }
        let dx = EDGE - l.card.x;
        let (w, h) = (EDGE + l.card.w + board_ui::M_SIDE, l.card.bottom() + GAP);
        let (hover, drag) = (self.board_hover.as_ref(), self.drag.as_ref());
        let Ok(frame) = self.renderer.render(w, h, geo.scale, TRANSPARENT, |p| p.translate(dx, 0.0, |p| paint_board(p, &l, &model, hover, &scroll, drag))) else {
            return;
        };
        drop(model);
        self.board_scroll = scroll;
        set_hand(Surface::Board, self.drag.is_none() && matches!(self.board_hover, Some(BoardHit::Card(..) | BoardHit::Filter(_) | BoardHit::Refresh | BoardHit::Close) | Some(BoardHit::Session(_))));
        let _ = self.board.show(&frame, geo.rect.left, geo.rect.top - frame.height as i32);
        self.board_geo = Some(BoardGeo { layout: l, dx, scale: geo.scale });
    }

    fn open_board(&mut self) {
        if self.flyout.visible() {
            self.close_flyout();
        }
        self.hide_tip();
        self.board_scroll = [0.0; 4];
        self.board_hover = None;
        self.press = None;
        self.drag = None;
        // Opening is the moment to want fresh cards.
        if self.board_file.as_ref().is_none_or(|b| now_ms() - b.updated_at > BOARD_STALE_MS) && !self.pending.iter().any(|p| p.task.is_none()) {
            self.send(Request::Refresh, None, String::new());
        }
        self.draw_board();
        self.draw_toasts();
    }

    fn close_board(&mut self) {
        if !self.board.visible() {
            return;
        }
        self.board.hide();
        self.board_geo = None;
        self.board_hover = None;
        self.press = None;
        self.drag = None;
        self.draw_toasts();
    }

    /// Board DIPs of a point in the board window's pixels.
    fn board_point(&self, x: i32, y: i32) -> Option<(f32, f32)> {
        let g = self.board_geo.as_ref()?;
        Some((x as f32 / g.scale - g.dx, y as f32 / g.scale))
    }

    fn board_move(&mut self, x: i32, y: i32) {
        let Some((bx, by)) = self.board_point(x, y) else { return };
        let folders = if self.drag.is_some() || self.press.is_some() { self.board_folders() } else { Vec::new() };
        let Some(g) = self.board_geo.as_ref() else { return };
        // A press that travels far enough on a card picks it up.
        if self.drag.is_none()
            && let Some(press) = &self.press
            && let Some(BoardHit::Card(column, key)) = &press.hit
            && (bx - press.x).hypot(by - press.y) > DRAG_SLOP
            && let Some((_, rect)) = g.layout.card_rect(key, &self.board_scroll)
        {
            let _ = rect;
            let mut size = (0.0, 0.0);
            let _ = self.renderer.render(1.0, 1.0, 1.0, TRANSPARENT, |p| size = board_ui::ghost_size(p));
            let from = self.board_file.as_ref().and_then(|b| b.state_column(key)).unwrap_or(*column);
            self.drag = Some(Drag { key: key.clone(), from, x: 0.0, y: 0.0, w: size.0, h: size.1, over: None });
            self.board_hover = None;
        }
        if let Some(press) = &self.press
            && let Some(d) = self.drag.as_mut()
        {
            // The chip rides just below and right of the pointer, like a
            // dragged file, so what lies under the pointer stays in sight.
            let _ = press.grab;
            (d.x, d.y) = board_ui::ghost_at(g.layout.card, bx, by, d.w, d.h);
            let over = g.layout.drop_at(bx, by, &self.board_scroll, &folders);
            // Back over its own column is no move at all.
            d.over = over.filter(|o| *o != Drop::Target(Target::Column(d.from)));
            self.board_dirty = true;
            return;
        }
        let hover = g.layout.hit(bx, by, &self.board_scroll);
        if hover != self.board_hover {
            self.board_hover = hover;
            self.board_dirty = true;
        }
    }

    fn board_down(&mut self, x: i32, y: i32) {
        let Some((bx, by)) = self.board_point(x, y) else { return };
        let Some(g) = self.board_geo.as_ref() else { return };
        let hit = g.layout.hit(bx, by, &self.board_scroll);
        let grab = match &hit {
            Some(BoardHit::Card(_, key)) => g.layout.card_rect(key, &self.board_scroll).map_or((0.0, 0.0), |(_, r)| (r.x - bx, r.y - by)),
            _ => (0.0, 0.0),
        };
        self.press = Some(Press { hit, x: bx, y: by, grab });
    }

    fn board_up(&mut self, x: i32, y: i32) {
        let press = self.press.take();
        if let Some(d) = self.drag.take() {
            self.board_dirty = true;
            match d.over {
                Some(Drop::Target(t)) => self.drop_card(&d.key, d.from, t),
                Some(Drop::Blocked(_)) => self.say(self.settings.language.board().cannot_drop.to_owned(), Tone::Error),
                None => {}
            }
            return;
        }
        let Some((bx, by)) = self.board_point(x, y) else { return };
        let Some(g) = self.board_geo.as_ref() else { return };
        let hit = g.layout.hit(bx, by, &self.board_scroll);
        let on_card = g.layout.card.contains(bx, by);
        // A click is a press and a release on the same thing.
        if press.as_ref().map(|p| &p.hit) != Some(&hit) {
            return;
        }
        match hit {
            Some(BoardHit::Close) => self.close_board(),
            Some(BoardHit::Refresh) => {
                self.send(Request::Refresh, None, String::new());
                self.say(self.settings.language.board().refreshing.to_owned(), Tone::Info);
            }
            Some(BoardHit::Filter(f)) => {
                self.board_filter = f;
                self.board_scroll = [0.0; 4];
                self.board_dirty = true;
            }
            Some(BoardHit::Card(_, key)) => {
                if let Some(url) = self.board_file.as_ref().and_then(|b| b.card(&key)).map(|c| c.url.clone()).filter(|u| !u.is_empty()) {
                    shell::open_url(&url);
                }
            }
            Some(BoardHit::Session(key)) => self.open_chat(&key),
            Some(BoardHit::NewChat(_)) => self.say(self.settings.language.board().hint.to_owned(), Tone::Info),
            None if !on_card => self.close_board(),
            None => {}
        }
    }

    fn board_wheel(&mut self, delta: i32) {
        let Some(g) = self.board_geo.as_ref() else { return };
        // The well under the pointer scrolls; outside any, the first that can.
        let (cx, cy) = cursor_over(self.board.hwnd()).and_then(|(x, y)| self.board_point(x, y)).unwrap_or((-1.0, -1.0));
        let Some(i) = (0..4).find(|&i| g.layout.lanes[i].contains(cx, cy)) else { return };
        let next = (self.board_scroll[i] - delta as f32 / 120.0 * SCROLL_STEP).clamp(0.0, g.layout.scroll_max[i]);
        if next != self.board_scroll[i] {
            self.board_scroll[i] = next;
            self.board_hover = None;
            self.board_dirty = true;
        }
    }

    fn drop_card(&mut self, key: &str, from: Column, target: Target) {
        let words = self.settings.language.board();
        match target {
            Target::Column(to) if to == from => {}
            Target::Column(to) => self.send(Request::Move { task: key.to_owned(), to }, Some(key), words.moving.to_owned()),
            t => self.send(Request::Assign { task: key.to_owned(), target: t }, Some(key), words.sending.to_owned()),
        }
    }

    /// Leave a request for the mod and wait for its reply.
    fn send(&mut self, r: Request, task: Option<&str>, label: String) {
        let now = now_ms();
        self.salt = self.salt.wrapping_add(0x9E37).wrapping_mul(31) ^ (now as u32);
        let id = request_id(now, self.salt);
        if self.mailbox.send(&id, now, &r).is_err() {
            self.say(self.settings.language.board().write_failed.to_owned(), Tone::Error);
            return;
        }
        self.pending.push(Pending { id, at: now, task: task.map(str::to_owned), label });
        self.timer(T_BOARD, BOARD_TICK_MS);
        self.board_dirty = true;
    }

    fn say(&mut self, text: String, tone: Tone) {
        self.status = Some((Status { text, tone }, now_ms()));
        self.board_dirty = true;
    }

    /// Replies in, timeouts out.
    fn read_replies(&mut self, now: i64) {
        if self.pending.is_empty() {
            return;
        }
        self.mailbox.sweep(now);
        let mut done = Vec::new();
        for p in &self.pending {
            if let Some(reply) = self.mailbox.reply(&p.id) {
                done.push((p.id.clone(), Some(reply)));
            } else if now - p.at > REQUEST_TIMEOUT_MS {
                done.push((p.id.clone(), None));
            }
        }
        for (id, reply) in done {
            self.mailbox.forget(&id);
            let quiet = self.pending.iter().any(|p| p.id == id && p.task.is_none());
            self.pending.retain(|p| p.id != id);
            match reply {
                // A refresh says nothing when it went well: the cards speak.
                Some(r) if r.ok && quiet => {}
                Some(r) => {
                    if r.ok
                        && let Some(a) = r.assignment()
                    {
                        self.seen_holders.retain(|b| b.task != a.task);
                        self.seen_holders.push(a.clone());
                        self.assignments.retain(|b| b.task != a.task);
                        self.assignments.push(a);
                    }
                    self.say(r.text, if r.ok { Tone::Ok } else { Tone::Error })
                }
                None => self.say(self.settings.language.board().timeout.to_owned(), Tone::Error),
            }
        }
        self.board_dirty = true;
    }

    fn on_taskbar_created(&mut self) {
        self.strip.forget();
        self.strip_key = None;
        self.hover = None;
        self.hide_tip();
        self.draw_all();
    }
}

/// Run `f` on the app if nobody holds it, then hand it every queued input.
/// While a modal loop (the menu) holds the app, timers simply skip a beat.
fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else { return };
        let Some(app) = guard.as_mut() else { return };
        f(app);
        while let Some(input) = crate::win::next_input() {
            app.on_input(input);
        }
        if std::mem::take(&mut app.board_dirty) && app.board.visible() {
            app.draw_board();
        }
    });
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let created = CREATED.get();
    match msg {
        WM_TIMER => with_app(|a| a.on_timer(wp.0)),
        WM_INPUT_READY => with_app(|_| {}),
        // SAFETY: a timer on our own window.
        WM_FOLDER_CHANGED => unsafe {
            SetTimer(hwnd, T_SOON, SOON_MS, None);
        },
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED => with_app(|a| {
            a.sync_theme();
            a.strip_key = None;
            a.draw_all();
        }),
        m if m == created && created != 0 => with_app(App::on_taskbar_created),
        // `taskkill` without /F, or anyone asking politely: leave the way the
        // menu's Quit does, so the strip is taken out of the taskbar before
        // the process ends. Killing the process instead was followed by
        // Explorer refusing child windows for minutes.
        // SAFETY: ends our own message loop.
        WM_CLOSE => unsafe { PostQuitMessage(0) },
        // Signing out or shutting down: Windows ends the process right after
        // this returns, so take the strip out of the taskbar now.
        WM_ENDSESSION if wp.0 != 0 => APP.with(|cell| {
            if let Ok(mut app) = cell.try_borrow_mut() {
                *app = None;
            }
        }),
        // SAFETY: default handling for our own window.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

/// Watch the status-band folder, `sessions/` and `acks/` from a helper
/// thread and nudge the host on every change. Not the whole tree: `diag/`
/// and the like change every few seconds and say nothing about sessions.
/// The 2 s timer covers folders that do not exist yet.
fn watch(dir: PathBuf, host: HWND) {
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::System::Threading::{INFINITE, WaitForMultipleObjects};
    let host = host.0 as isize;
    let _ = std::thread::Builder::new().name("claudibeat-watch".into()).spawn(move || {
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;
        loop {
            // SAFETY: change notification handles opened, waited on and closed on this thread.
            unsafe {
                let handles: Vec<HANDLE> = [dir.clone(), dir.join("sessions"), dir.join("acks")]
                    .iter()
                    .filter_map(|d| FindFirstChangeNotificationW(&windows::core::HSTRING::from(d.as_os_str()), false, filter).ok())
                    .collect();
                let mut alive = !handles.is_empty();
                while alive {
                    let which = WaitForMultipleObjects(&handles, false, INFINITE).0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
                    let Some(&h) = handles.get(which) else { break };
                    if PostMessageW(HWND(host as *mut _), WM_FOLDER_CHANGED, WPARAM(0), LPARAM(0)).is_err() {
                        for &h in &handles {
                            let _ = FindCloseChangeNotification(h);
                        }
                        return;
                    }
                    alive = FindNextChangeNotification(h).is_ok();
                }
                for &h in &handles {
                    let _ = FindCloseChangeNotification(h);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

/// Ask the running claudibeat to quit and wait up to `timeout_ms` for its
/// process to end. `true` once none runs.
///
/// `WM_QUIT` goes to its UI thread, so it leaves exactly as through the
/// menu's Quit (and builds older than the `WM_CLOSE` handling quit too).
pub fn quit_running(timeout_ms: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: a lookup by class name, a posted message, and a wait on a
    // process handle that is closed before returning.
    unsafe {
        let Some(host) = FindWindowW(HOST_CLASS, None).ok().filter(|h| !h.is_invalid()) else { return true };
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(host, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let asked = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)).is_ok();
        let ended = asked && WaitForSingleObject(process, timeout_ms) == WAIT_OBJECT_0;
        let _ = CloseHandle(process);
        // The guard leaves with its child: wait for it too, so the exe is free.
        let until = std::time::Instant::now() + std::time::Duration::from_millis(u64::from(timeout_ms));
        while ended && crate::guard::guard_running() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        ended
    }
}

/// Hold `Local\claudibeat` for the life of the process; `None` when another
/// claudibeat already runs.
pub fn single_instance() -> Option<HANDLE> {
    // SAFETY: creates or opens a named mutex; the handle is kept until exit.
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\claudibeat")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(h)
    }
}

pub fn run() {
    // SAFETY: Win32 start-up and the message loop, all on this thread.
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSW { lpfnWndProc: Some(host_proc), hInstance: instance, lpszClassName: HOST_CLASS, ..Default::default() };
        RegisterClassW(&wc);
        strip_window::register(instance);
        popup::register(instance);

        // Never shown, but a top-level window hears Explorer's broadcasts.
        let Ok(host) = CreateWindowExW(WS_EX_TOOLWINDOW, HOST_CLASS, w!("claudibeat host"), WS_POPUP, 0, 0, 0, 0, None, None, instance, None) else {
            return;
        };
        crate::win::set_host(host);
        let created = taskbar::created_msg();
        CREATED.set(created);
        // Explorer's broadcast still arrives if claudibeat runs elevated.
        let _ = ChangeWindowMessageFilterEx(host, created, MSGFLT_ALLOW, None);

        let Some(app) = App::new(instance, host) else {
            let _ = DestroyWindow(host);
            return;
        };
        let dir = app.store.dir().to_path_buf();
        let _ = app.store.write_tray(now_ms(), app.settings.quiet_helpers);
        app.mailbox.sweep(now_ms());
        APP.set(Some(app));
        with_app(App::refresh);

        SetTimer(host, T_REFRESH, REFRESH_MS, None);
        SetTimer(host, T_MEMORY, MEMORY_MS, None);
        SetTimer(host, T_TRAY, TRAY_MS, None);
        watch(dir, host);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.set(None);
        let _ = DestroyWindow(host);
    }
}
