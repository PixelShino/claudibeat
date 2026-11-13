//! The running program: one thread, one message loop.
//!
//! A hidden top-level host window owns the timers and hears `TaskbarCreated`
//! (Explorer broadcasts it to top-level windows only). The strip is a child
//! of the taskbar and dies with it; the host simply builds a new one. Window
//! procedures queue [`Input`]s; the host hands them to [`App`] with its state
//! borrowed only here, never inside another window procedure.

use crate::limits::{LimitView, limit_views, strip_limit};
use crate::memory::{self, MemGroups};
use crate::merge::{Row, counts, groups, merge_rows, open_target};
use crate::notify::{Notifier, ToastKind, ToastStack};
use crate::render::flyout::{FlyoutHit, FlyoutLayout, FlyoutModel, M_BOTTOM, M_SIDE, layout_flyout, paint_flyout};
use crate::render::strip::{STRIP_H, StripItem, StripLayout, StripModel, layout_strip, paint_strip};
use crate::render::toast::{T_SIDE, T_TOP, TOAST_W, ToastModel, layout_toast, paint_toast, paint_tooltip, toast_model, tooltip_size};
use crate::render::{Rect, Renderer, TRANSPARENT};
use crate::settings::Settings;
use crate::store::Store;
use crate::text::{local_tz_offset_min, tip_done, tip_limit, tip_memory, tip_waiting, tip_working};
use crate::win::popup::{self, Popup};
use crate::win::strip_window::{self, StripWindow};
use crate::win::{Action, Input, Surface, WM_FOLDER_CHANGED, WM_INPUT_READY, cursor_over, set_hand, shell, taskbar};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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
        let work_top = if GetMonitorInfoW(MonitorFromWindow(bar, MONITOR_DEFAULTTOPRIMARY), &mut mi).as_bool() { mi.rcWork.top } else { 0 };
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

struct ToastHit {
    index: usize,
    card: Rect,
    close: Rect,
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
    fly_hover: Option<String>,

    toasts: Popup,
    toast_geo: Option<ToastGeo>,
    toast_hovered: bool,
    toast_hover_close: Option<usize>,
    toast_timer: bool,
}

impl App {
    fn new(instance: HINSTANCE, host: HWND) -> Option<App> {
        let settings_path = Settings::default_path();
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
            toasts: Popup::new(instance, Surface::Toasts)?,
            toast_geo: None,
            toast_hovered: false,
            toast_hover_close: None,
            toast_timer: false,
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
        self.limits = limit_views(snap.account.as_ref(), now);
        self.busy = shell::is_busy();

        let events = self.notifier.observe(&self.rows, now);
        let needing: HashSet<String> = self.rows.iter().filter(|r| r.needs_you).map(|r| r.key.clone()).collect();
        self.stack.retain_needing(&needing);
        if let Some(first) = events.first() {
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

    fn strip_model(&self) -> StripModel {
        let c = counts(&self.rows);
        StripModel {
            waiting: c.waiting,
            done: c.done,
            working: c.working,
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
    }

    fn draw_all(&mut self) {
        self.sync_pointer();
        self.draw_strip();
        if self.flyout.visible() {
            self.draw_flyout();
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
        let (hover, scroll) = (self.fly_hover.as_deref(), self.scroll);
        let Ok(frame) = self.renderer.render(w, h, geo.scale, TRANSPARENT, |p| p.translate(dx, 0.0, |p| paint_flyout(p, &l, &model, hover, scroll))) else {
            return;
        };
        let _ = self.flyout.show(&frame, geo.rect.left, geo.rect.top - frame.height as i32);
        self.fly_geo = Some(FlyGeo { layout: l, dx, scale: geo.scale });
    }

    fn draw_toasts(&mut self) {
        let geo = geometry();
        let (Some(geo), false, false, false) = (geo, self.stack.items.is_empty(), self.flyout.visible(), self.busy) else {
            self.toasts.hide();
            self.toast_geo = None;
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
            }
            Action::ToggleAutostart => shell::autostart_set(!shell::autostart_get()),
            Action::SetLanguage(lang) => {
                self.settings.language = lang;
                let _ = self.settings.save(&self.settings_path);
                self.draw_all();
            }
            // SAFETY: ends our own message loop.
            Action::Quit => unsafe { PostQuitMessage(0) },
        }
    }

    /// Open the chat, mark it seen (shared with the strip inside Claude),
    /// and let its toasts go.
    fn open_chat(&mut self, key: &str) {
        let Some(url) = self.rows.iter().find(|r| r.key == key).and_then(open_target).map(str::to_owned) else { return };
        if self.flyout.visible() {
            self.close_flyout();
        }
        let now = now_ms();
        shell::open_url(&url);
        let _ = self.store.write_ack(key, now);
        self.notifier.opened(key, now);
        self.refresh();
    }

    fn open_flyout(&mut self) {
        self.hide_tip();
        self.expanded = [false; 3];
        self.scroll = 0.0;
        self.fly_hover = None;
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
                if let Some(a) = strip_window::menu(self.host, pending, self.settings.sound, shell::autostart_get(), self.settings.language) {
                    self.act(a);
                }
            }

            Input::Move(Surface::Flyout, x, y) => {
                let hover = match self.flyout_at(x, y).0 {
                    Some(FlyoutHit::Row(key)) => Some(key),
                    _ => None,
                };
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
                (None, false) => self.close_flyout(),
                (None, true) => {}
            },
            Input::Wheel(Surface::Flyout, delta) => {
                let max = self.fly_geo.as_ref().map_or(0.0, |g| g.layout.scroll_max);
                let next = (self.scroll - delta as f32 / 120.0 * SCROLL_STEP).clamp(0.0, max);
                if next != self.scroll {
                    self.scroll = next;
                    self.fly_hover = None;
                    self.draw_flyout();
                }
            }
            Input::Escape(Surface::Flyout) | Input::Deactivated(Surface::Flyout) if self.flyout.visible() => self.close_flyout(),

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
                let _ = self.store.write_tray(now_ms());
            }
            T_TIP => {
                self.kill(T_TIP);
                if let (Some(item), false) = (self.hover, self.flyout.visible()) {
                    self.show_tip(item);
                }
            }
            T_TOAST => self.tick_toasts(),
            _ => {}
        }
    }

    fn tick_toasts(&mut self) {
        let close = self.toast_hover_close;
        self.sync_pointer();
        if close != self.toast_hover_close {
            self.draw_toasts();
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
            a.strip_key = None;
            a.draw_all();
        }),
        m if m == created && created != 0 => with_app(App::on_taskbar_created),
        // SAFETY: default handling for our own window.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

/// Watch the status-band folder from a helper thread and nudge the host on
/// every change. The 2 s timer covers a folder that does not exist yet.
fn watch(dir: PathBuf, host: HWND) {
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    let host = host.0 as isize;
    let _ = std::thread::Builder::new().name("claudibeat-watch".into()).spawn(move || {
        let path = windows::core::HSTRING::from(dir.as_os_str());
        loop {
            // SAFETY: a change notification handle used and closed on this thread.
            unsafe {
                if let Ok(h) = FindFirstChangeNotificationW(&path, true, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE) {
                    while WaitForSingleObject(h, INFINITE) == WAIT_OBJECT_0 {
                        if PostMessageW(HWND(host as *mut _), WM_FOLDER_CHANGED, WPARAM(0), LPARAM(0)).is_err() {
                            let _ = FindCloseChangeNotification(h);
                            return;
                        }
                        if FindNextChangeNotification(h).is_err() {
                            break;
                        }
                    }
                    let _ = FindCloseChangeNotification(h);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
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
        let _ = app.store.write_tray(now_ms());
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
