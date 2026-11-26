//! `claudibeat --render <dir> [--bg <png>] [--lang en|ru] [--theme light|dark]`: every surface
//! drawn to PNG with demo data, to compare against the mock-ups and to make
//! the README pictures without touching the desktop.

use crate::board::{Assignment, BoardFile, Card, Column, Pending, Target, project_folders};
use crate::limits::{LimitView, LimitWindow};
use crate::memory::MemGroups;
use crate::merge::{Row, groups};
use crate::model::SessionState;
use crate::notify::{ToastEvent, ToastKind};
use crate::render::board::{BoardHit, BoardModel, Drag, Drop, Status, Tone, ghost_at, ghost_size, layout_board, paint_board};
use crate::render::flyout::{FlyoutHit, FlyoutModel, FlyoutView, Prefs, layout_flyout, paint_flyout};
use crate::render::strip::{STRIP_H, StripItem, StripModel, layout_strip, paint_strip};
use crate::render::toast::{layout_toast, paint_toast, paint_tooltip, toast_model, tooltip_size};
use crate::render::{Color, Painter, Rect, Renderer, TRANSPARENT, Theme};
use crate::text::Lang;
use std::path::Path;

const SCALE: f32 = 2.0;
const CANVAS_W: f32 = 960.0;
const MIN: i64 = 60_000;
/// 2026-11-13 18:11 at UTC+3, for stable demo times.
const NOW: i64 = 1_794_582_660_000;
const TZ: i32 = 180;

struct StripSample {
    name: &'static str,
    model: StripModel,
    hover: Option<StripItem>,
}

fn strip_samples() -> Vec<StripSample> {
    let base = StripModel { waiting: 1, done: 2, working: 4, tasks: None, mem_percent: 76, mem_alert: false, limit: Some((66, 0.66)) };
    vec![
        StripSample { name: "strip-wait", model: base, hover: None },
        StripSample { name: "strip-calm", model: StripModel { waiting: 0, done: 0, working: 2, mem_percent: 59, ..base }, hover: None },
        StripSample { name: "strip-12", model: StripModel { waiting: 12, done: 3, mem_percent: 91, mem_alert: true, ..base }, hover: None },
        StripSample { name: "strip-hover", model: base, hover: Some(StripItem::Working) },
        StripSample { name: "strip-nolimit", model: StripModel { limit: None, ..base }, hover: None },
    ]
}

fn demo_row(key: &str, title: &str, project: &str, state: SessionState, ask: Option<&str>, doing: Option<&str>, mins: i64) -> Row {
    Row {
        key: key.into(),
        desktop_id: Some(key.into()),
        title: title.into(),
        project: project.into(),
        cwd: String::new(),
        link: Some(format!("claude://claude.ai/epitaxy/{key}")),
        state,
        ask: ask.map(Into::into),
        since: NOW - mins * MIN,
        doing: doing.map(Into::into),
        needs_you: state != SessionState::Working,
        last_activity_at: NOW - mins * MIN,
        parent: None,
    }
}

/// Demo sessions: invented titles, safe to publish. Each text comes in
/// English and Russian.
fn demo_rows(extra_waiting: usize, lang: Lang) -> Vec<Row> {
    use SessionState::*;
    let t = |en: &'static str, ru: &'static str| if lang == Lang::En { en } else { ru };
    let mut rows = vec![
        demo_row("w1", t("Payments: refunds", "Платёжный модуль: возвраты"), "billing", Waiting, Some(t("Keep the old API until the release?", "Оставить старый API до релиза?")), None, 7),
        demo_row("d1", t("Coffee shop site: order form", "Лендинг кофейни: форма заказа"), "coffee-site", Done, None, None, 3),
        demo_row("d2", t("Price list parser", "Парсер прайс-листов"), "price-parser", Done, None, None, 33),
        demo_row("r1", t("Move the database to Postgres", "Миграция базы на Postgres"), "core-api", Working, None, Some(t("running migration tests", "запускает тесты миграций")), 10),
        demo_row("r2", t("Support bot: refund flow", "Бот поддержки: сценарии"), "support-bot", Working, None, Some(t("editing src/flows/refund.ts", "правит src/flows/refund.ts")), 1),
        demo_row("r3", t("October traffic report", "Отчёт по трафику за октябрь"), "analytics", Working, None, Some(t("drawing charts", "строит графики")), 4),
        demo_row("r4", t("Auth refactor", "Рефакторинг авторизации"), "core-api", Working, None, Some(t("reading auth/session.rs", "читает auth/session.rs")), 2),
    ];
    for i in 0..extra_waiting {
        let title = match lang {
            Lang::En => format!("Backlog task #{}", i + 2),
            Lang::Ru => format!("Задача из бэклога №{}", i + 2),
        };
        rows.push(demo_row(&format!("x{i}"), &title, "backlog", Waiting, Some(t("Which option should I take?", "Какой вариант взять?")), None, 20 + i as i64));
    }
    rows
}

fn demo_mem() -> MemGroups {
    let gib = |x: f64| (x * (1u64 << 30) as f64) as u64;
    MemGroups { claude: gib(6.2), chrome: gib(0.7), python: gib(0.6), other: gib(16.7), used: gib(24.2), total: gib(31.8) }
}

fn demo_limits() -> Vec<LimitView> {
    vec![
        LimitView { window: LimitWindow::FiveHour, used: 23, remaining: 77, resets_at: Some(NOW + 149 * MIN) },
        LimitView { window: LimitWindow::Week, used: 66, remaining: 34, resets_at: Some(1_794_754_800_000) },
    ]
}

fn io_err(e: std::io::Error) -> windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(-1), e.to_string())
}

/// A slice of desktop with the taskbar at the bottom and the strip on it.
fn scene(p: &Painter, h: f32, bg: Option<&Path>, strip: &StripModel, hover: Option<StripItem>, theme: Theme) {
    // A wallpaper-grey desktop over the taskbar, light or dark as Windows draws it.
    let (desk, bar) = match theme {
        Theme::Light => (Color(0xFFD2_D4D8), Color(0xFFEE_EEF0)),
        Theme::Dark => (Color(0xFF3A_3D42), Color(0xFF1C_1C1C)),
    };
    p.fill_rect(Rect::new(0.0, 0.0, CANVAS_W, h), desk);
    let bar_y = h - STRIP_H;
    match bg {
        Some(bg) => p.translate(0.0, bar_y, |p| p.image(bg, Rect::new(0.0, 0.0, CANVAS_W, STRIP_H))),
        None => p.fill_rect(Rect::new(0.0, bar_y, CANVAS_W, STRIP_H), bar),
    }
    p.translate(0.0, bar_y, |p| {
        let l = layout_strip(strip, p);
        paint_strip(p, &l, strip, hover);
    });
}

pub fn render_samples(dir: &Path, bg: Option<&Path>, lang: Lang, theme: Theme) -> windows::core::Result<()> {
    std::fs::create_dir_all(dir).map_err(io_err)?;
    let r = Renderer::new()?;
    r.set_theme(theme);
    for s in strip_samples() {
        let frame = r.render(CANVAS_W, STRIP_H, SCALE, TRANSPARENT, |p| {
            if let Some(bg) = bg {
                p.image(bg, Rect::new(0.0, 0.0, CANVAS_W, STRIP_H));
            }
            let l = layout_strip(&s.model, p);
            paint_strip(p, &l, &s.model, s.hover);
        })?;
        r.save_png(&frame, &dir.join(format!("{}.png", s.name)))?;
    }

    let strip = strip_samples()[0].model;
    let w1 = FlyoutHit::Row("w1".into());
    let prefs = Prefs { dnd: false, sound: true, autostart: true, quiet_helpers: true };
    struct Sample {
        name: &'static str,
        extra: usize,
        expanded: [bool; 3],
        max_h: f32,
        view: FlyoutView,
        hover: Option<FlyoutHit>,
        prefs: Prefs,
    }
    let list = |name, extra, expanded, max_h, hover| Sample { name, extra, expanded, max_h, view: FlyoutView::Sessions, hover, prefs };
    let flyouts = [
        list("flyout", 0, [false; 3], 2_000.0, Some(w1.clone())),
        list("flyout-expanded", 0, [false, false, true], 2_000.0, Some(w1.clone())),
        list("flyout-scroll", 11, [true, false, false], 640.0, Some(w1)),
        Sample { prefs: Prefs { dnd: true, ..prefs }, ..list("flyout-dnd", 0, [false; 3], 2_000.0, Some(FlyoutHit::Gear)) },
        Sample { view: FlyoutView::Settings, ..list("flyout-settings", 0, [false; 3], 2_000.0, Some(FlyoutHit::Sound)) },
    ];
    for Sample { name, extra, expanded, max_h, view, hover, prefs } in flyouts {
        let rows = demo_rows(extra, lang);
        let m = FlyoutModel { groups: groups(&rows), total: 48, expanded, mem: demo_mem(), limits: demo_limits(), now: NOW, tz: TZ, lang, view, prefs };
        let mut size = (0.0, 0.0);
        r.render(1.0, 1.0, 1.0, TRANSPARENT, |p| {
            let l = layout_flyout(&m, p, max_h);
            size = (l.width, l.height);
        })?;
        let h = size.1 + STRIP_H;
        let frame = r.render(CANVAS_W, h, SCALE, TRANSPARENT, |p| {
            scene(p, h, bg, &strip, None, theme);
            let l = layout_flyout(&m, p, max_h);
            p.translate(8.0 - l.card.x, h - STRIP_H - 12.0 - l.card.bottom(), |p| {
                paint_flyout(p, &l, &m, hover.as_ref(), if name == "flyout-scroll" { 40.0 } else { 0.0 });
            });
        })?;
        r.save_png(&frame, &dir.join(format!("{name}.png")))?;
    }

    let rows = demo_rows(0, lang);
    let mut no_link = rows.clone();
    no_link[1].link = None;
    let toasts: [(&str, Vec<ToastEvent>, &[Row]); 3] = [
        ("toasts", vec![ev(ToastKind::Waiting, &["w1"]), ev(ToastKind::Done, &["d1"])], &rows),
        ("toast-batch", vec![ev(ToastKind::MoreWaiting, &["w1", "x0"]), ev(ToastKind::DoneBatch, &["d1", "d2", "r1"])], &rows),
        ("toast-nolink", vec![ev(ToastKind::Done, &["d1"])], &no_link),
    ];
    for (name, events, data) in toasts {
        let h = 420.0;
        let frame = r.render(CANVAS_W, h, SCALE, TRANSPARENT, |p| {
            scene(p, h, bg, &strip, None, theme);
            // Lay out from the strip upwards, then paint top-down as the app
            // does, so no card's shadow falls on the card below it.
            let mut bottom = h - STRIP_H - 12.0;
            let mut placed = Vec::new();
            for e in events.iter().rev() {
                let m = toast_model(e, data, NOW, lang);
                let l = layout_toast(&m, p);
                let dy = bottom - l.card.bottom();
                bottom -= l.card.h + 8.0;
                placed.push((m, l, dy));
            }
            for (m, l, dy) in placed.iter().rev() {
                p.translate(8.0 - l.card.x, *dy, |p| paint_toast(p, l, m, false));
            }
        })?;
        r.save_png(&frame, &dir.join(format!("{name}.png")))?;
    }

    let h = 120.0;
    let frame = r.render(CANVAS_W, h, SCALE, TRANSPARENT, |p| {
        scene(p, h, bg, &strip, Some(StripItem::Working), theme);
        let text = crate::text::tip_working(lang, 4);
        let (_, th) = tooltip_size(p, &text);
        p.translate(70.0, h - STRIP_H - th + 2.0, |p| paint_tooltip(p, &text));
    })?;
    r.save_png(&frame, &dir.join("tooltip.png"))?;
    render_board_samples(&r, dir, bg, lang, theme)
}

/// Invented Plane work items, safe to publish, in both languages.
fn demo_board(lang: Lang) -> BoardFile {
    let t = |en: &'static str, ru: &'static str| if lang == Lang::En { en } else { ru };
    let card = |key: &str, project: &str, priority: &str, title: &str| Card {
        key: key.into(),
        title: title.into(),
        project: project.into(),
        group: String::new(),
        state_name: String::new(),
        priority: priority.into(),
        description: String::new(),
        url: format!("https://plane.example/browse/{key}/"),
    };
    BoardFile {
        v: 1,
        updated_at: NOW - 2 * MIN,
        error: None,
        projects: vec!["WEB".into(), "API".into(), "OPS".into()],
        todo: vec![
            card("WEB-41", "WEB", "urgent", t("Checkout: the promo code field eats the last digit", "Оформление заказа: поле промокода съедает последнюю цифру")),
            card("API-17", "API", "high", t("Rate limits for the public API", "Лимиты запросов для публичного API")),
            card("WEB-44", "WEB", "medium", t("Dark theme for the account pages", "Тёмная тема для личного кабинета")),
            card("OPS-9", "OPS", "", t("Nightly database backup to a second region", "Ночной бэкап базы во второй регион")),
            card("WEB-46", "WEB", "", t("Compress hero images on the landing", "Сжать картинки на главной")),
            card("API-19", "API", "medium", t("Webhook retries with backoff", "Повторы вебхуков с паузой")),
        ],
        doing: vec![
            card("API-12", "API", "high", t("Move the database to Postgres", "Миграция базы на Postgres")),
            card("WEB-38", "WEB", "", t("Support bot: refund flow", "Бот поддержки: сценарий возврата")),
        ],
        done_today: vec![
            card("OPS-7", "OPS", "", t("Renew the TLS certificates", "Продлить TLS-сертификаты")),
            card("WEB-35", "WEB", "medium", t("Contact form sends twice", "Форма обратной связи отправляет дважды")),
        ],
    }
}

type BoardSample = (&'static str, Option<String>, Option<BoardHit>, Option<(&'static str, Drop)>, Option<Status>);

fn render_board_samples(r: &Renderer, dir: &Path, bg: Option<&Path>, lang: Lang, theme: Theme) -> windows::core::Result<()> {
    let board = demo_board(lang);
    let mut rows = demo_rows(0, lang);
    let cwds = ["D:\\Code\\billing", "D:\\Code\\coffee-site", "", "D:\\Code\\core-api", "D:\\Code\\support-bot", "D:\\Code\\analytics", "D:\\Code\\core-api"];
    for (row, cwd) in rows.iter_mut().zip(cwds) {
        row.cwd = cwd.into();
        row.key = format!("local_{}", row.key);
    }
    let assignments = vec![
        Assignment { task: "API-12".into(), session: "local_r1".into(), session_title: rows[3].title.clone(), at: NOW },
        Assignment { task: "WEB-38".into(), session: "local_r2".into(), session_title: rows[4].title.clone(), at: NOW },
    ];
    let seen: Vec<(String, String, i64)> = rows.iter().map(|r| (r.cwd.clone(), r.project.clone(), r.last_activity_at)).collect();
    let folders: Vec<(String, String)> = project_folders(&seen, 5).into_iter().chain(std::iter::once((String::new(), String::new()))).collect();
    let pending = vec![Pending { id: "cb-1".into(), at: NOW, task: Some("API-19".into()), label: lang.board().sending.into() }];
    let strip = StripModel { tasks: Some(2), ..strip_samples()[0].model };
    let canvas_w = 1200.0;
    let samples: [BoardSample; 4] = [
        ("board", None, Some(BoardHit::Card(Column::Todo, "API-17".into())), None, Some(Status { text: format!("WEB-30 → «{}»", rows[5].title), tone: Tone::Ok })),
        ("board-drag-agent", None, None, Some(("WEB-41", Drop::Target(Target::Session("local_d1".into())))), None),
        ("board-drag-new", Some("WEB".into()), None, Some(("WEB-41", Drop::Target(Target::NewChat { cwd: folders[0].0.clone(), project: folders[0].1.clone() }))), None),
        ("board-drag-column", None, None, Some(("OPS-9", Drop::Target(Target::Column(Column::Done)))), None),
    ];
    for (name, filter, hover, drag, status) in samples {
        let agents: Vec<&Row> = rows.iter().collect();
        let m = BoardModel { board: Some(&board), agents, assignments: &assignments, pending: &pending, filter, folders: folders.clone(), now: NOW, lang, status };
        let max_h = 660.0;
        let mut size = (0.0, 0.0);
        r.render(1.0, 1.0, 1.0, TRANSPARENT, |p| {
            let l = layout_board(&m, p, canvas_w, max_h);
            size = (l.width, l.height);
        })?;
        let h = size.1 + STRIP_H;
        let frame = r.render(canvas_w, h, SCALE, TRANSPARENT, |p| {
            let (desk, bar) = match theme {
                Theme::Light => (Color(0xFFD2_D4D8), Color(0xFFEE_EEF0)),
                Theme::Dark => (Color(0xFF3A_3D42), Color(0xFF1C_1C1C)),
            };
            p.fill_rect(Rect::new(0.0, 0.0, canvas_w, h), desk);
            match bg {
                Some(bg) => p.translate(0.0, h - STRIP_H, |p| p.image(bg, Rect::new(0.0, 0.0, canvas_w, STRIP_H))),
                None => p.fill_rect(Rect::new(0.0, h - STRIP_H, canvas_w, STRIP_H), bar),
            }
            p.translate(0.0, h - STRIP_H, |p| {
                let l = layout_strip(&strip, p);
                paint_strip(p, &l, &strip, Some(StripItem::Tasks));
            });
            let l = layout_board(&m, p, canvas_w, max_h);
            let scroll = [0.0; 4];
            let drag = drag.as_ref().and_then(|(key, over)| {
                let (from, rect) = l.card_rect(key, &scroll)?;
                let spot = match over {
                    Drop::Target(Target::Session(k)) => l.sessions.iter().find(|s| &s.key == k).map(|s| s.rect),
                    Drop::Target(Target::NewChat { .. }) => l.folders.first().map(|(_, r)| *r),
                    Drop::Target(Target::Column(c)) => Some(l.lanes[c.index()]),
                    Drop::Blocked(_) => None,
                }?;
                // The pointer over its target, the chip just below and right.
                let _ = rect;
                let (w, gh) = ghost_size(p);
                let (x, y) = ghost_at(l.card, spot.x + spot.w * 0.4, spot.y + spot.h * 0.5, w, gh);
                Some(Drag { key: (*key).into(), from, x, y, w, h: gh, over: Some(over.clone()) })
            });
            p.translate(8.0 - l.card.x, h - STRIP_H - 12.0 - l.card.bottom(), |p| paint_board(p, &l, &m, hover.as_ref(), &scroll, drag.as_ref()));
        })?;
        r.save_png(&frame, &dir.join(format!("{name}.png")))?;
    }
    Ok(())
}

fn ev(kind: ToastKind, keys: &[&str]) -> ToastEvent {
    ToastEvent { kind, keys: keys.iter().map(|k| k.to_string()).collect(), at: NOW, count: keys.len() }
}

/// `claudibeat --dump <file>`: today's counts, memory and limits as JSON, to
/// check the numbers without any window.
pub fn dump(path: &Path) -> std::io::Result<()> {
    use crate::merge::{counts, merge_rows};
    let mut store = crate::store::Store::new(crate::store::Store::default_dir());
    let snap = store.read();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let c = counts(&merge_rows(&snap.index, &snap.files, &snap.acks, now));
    let mem = crate::memory::sample();
    let limits: Vec<serde_json::Value> = crate::limits::limit_views(snap.account.as_ref(), now)
        .iter()
        .map(|l| {
            let window = match l.window {
                LimitWindow::FiveHour => "five_hour",
                LimitWindow::Week => "seven_day",
            };
            serde_json::json!({ "window": window, "used": l.used, "remaining": l.remaining, "resetsAt": l.resets_at })
        })
        .collect();
    let value = serde_json::json!({
        "counts": { "waiting": c.waiting, "done": c.done, "working": c.working, "total": c.total },
        "mem": {
            "claude": mem.claude, "chrome": mem.chrome, "python": mem.python, "other": mem.other,
            "used": mem.used, "total": mem.total, "percent": mem.percent(),
        },
        "limits": limits,
    });
    std::fs::write(path, serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?)
}
