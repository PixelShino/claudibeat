//! Every word claudibeat shows, in English (the default) and Russian, with
//! the right plural forms, decimal marks, quotes and weekday names.

use crate::limits::{LimitView, LimitWindow};
use serde::{Deserialize, Serialize};

const GIB: f64 = (1u64 << 30) as f64;
const DAY_MS: i64 = 86_400_000;
// 1970-01-01 was a Thursday.
const WEEKDAYS_EN: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const WEEKDAYS_RU: [&str; 7] = ["чт", "пт", "сб", "вс", "пн", "вт", "ср"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Ru,
}

/// The fixed words of one language.
pub struct Words {
    pub flyout_title: &'static str,
    /// Group headings: waiting, done, working.
    pub groups: [&'static str; 3],
    /// A waiting session's label when it asked nothing in words.
    pub waiting_one: &'static str,
    pub waiting_many: &'static str,
    pub done: &'static str,
    pub empty: &'static str,
    pub memory: &'static str,
    /// Memory rows: name and a quieter note after it.
    pub mem_rows: [(&'static str, &'static str); 4],
    pub limits: &'static str,
    pub open_chat: &'static str,
    pub show_all: &'static str,
    pub no_folder: &'static str,
    pub limit_unknown: &'static str,
    pub menu_seen: &'static str,
    pub menu_sound: &'static str,
    pub menu_autostart: &'static str,
    pub menu_language: &'static str,
    pub menu_quit: &'static str,
    pub settings: &'static str,
    pub dnd: &'static str,
    pub dnd_note: &'static str,
    pub sound_note: &'static str,
    pub helpers: &'static str,
    pub helpers_note: &'static str,
    pub autostart_note: &'static str,
}

const EN: Words = Words {
    flyout_title: "Claude sessions",
    groups: ["Waiting for you", "Done, not opened", "Working"],
    waiting_one: "Waiting for you",
    waiting_many: "Waiting for you",
    done: "Done",
    empty: "Nobody is waiting or working",
    memory: "Memory",
    mem_rows: [("Claude", "with MCP and terminals"), ("Chrome", ""), ("Python", "outside Claude"), ("Other and system", "")],
    limits: "Limits",
    open_chat: "Open chat",
    show_all: "Show all",
    no_folder: "no folder",
    limit_unknown: "Limit not known yet",
    menu_seen: "Mark all as seen",
    menu_sound: "Notification sound",
    menu_autostart: "Start with Windows",
    menu_language: "Language",
    menu_quit: "Quit",
    settings: "Settings",
    dnd: "Do not disturb",
    dnd_note: "No toasts or sound, the strip still counts",
    sound_note: "A soft chime with each toast",
    helpers: "Quiet helper chats",
    helpers_note: "Chats started by other chats stay hidden",
    autostart_note: "Appears on the taskbar after you sign in",
};

const RU: Words = Words {
    flyout_title: "Сессии Claude",
    groups: ["Ждут ответа", "Готово, не открыто", "В работе"],
    waiting_one: "Ждёт ответа",
    waiting_many: "Ждут ответа",
    done: "Готово",
    empty: "Никто не ждёт и не работает",
    memory: "Память",
    mem_rows: [("Claude", "с MCP и терминалами"), ("Chrome", ""), ("Python", "вне Claude"), ("Остальное и система", "")],
    limits: "Лимиты",
    open_chat: "Открыть чат",
    show_all: "Показать все",
    no_folder: "без папки",
    limit_unknown: "Лимит пока неизвестен",
    menu_seen: "Отметить всё просмотренным",
    menu_sound: "Звук уведомлений",
    menu_autostart: "Запускать вместе с Windows",
    menu_language: "Язык",
    menu_quit: "Выход",
    settings: "Настройки",
    dnd: "Не беспокоить",
    dnd_note: "Без уведомлений и звука, счётчики работают",
    sound_note: "Мягкий сигнал с каждым уведомлением",
    helpers: "Тихие помощники",
    helpers_note: "Чаты, запущенные другими чатами, не видны",
    autostart_note: "Появляется на панели задач после входа",
};

impl Lang {
    pub fn words(self) -> &'static Words {
        match self {
            Lang::En => &EN,
            Lang::Ru => &RU,
        }
    }

    /// The language's own name, as its menu item shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }
}

/// Russian plural: 1, 21, 101 → one; 2–4, 22–24 → few; the rest, 11–14 included → many.
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let (last, last_two) = (n % 10, n % 100);
    if (11..=14).contains(&last_two) {
        many
    } else if last == 1 {
        one
    } else if (2..=4).contains(&last) {
        few
    } else {
        many
    }
}

fn sessions(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{n} {}", if n == 1 { "session" } else { "sessions" }),
        Lang::Ru => format!("{n} {}", plural(n, "сессия", "сессии", "сессий")),
    }
}

pub fn tip_waiting(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{} waiting for you", sessions(lang, n)),
        Lang::Ru => format!("{} {} ответа", sessions(lang, n), plural(n, "ждёт", "ждут", "ждут")),
    }
}

pub fn tip_done(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{} finished, not opened yet", sessions(lang, n)),
        Lang::Ru => {
            let verb = plural(n, "закончила", "закончили", "закончили");
            let open = plural(n, "открыта", "открыты", "открыты");
            format!("{} {verb}, не {open}", sessions(lang, n))
        }
    }
}

pub fn tip_working(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{} working", sessions(lang, n)),
        Lang::Ru => format!("{} в работе", sessions(lang, n)),
    }
}

/// Title of one toast for several sessions that finished together.
pub fn title_done_batch(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{} finished", sessions(lang, n)),
        Lang::Ru => format!("{} {}", sessions(lang, n), plural(n, "закончила", "закончили", "закончили")),
    }
}

/// Title of the summary toast once every place holds someone waiting.
pub fn title_more_waiting(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{n} more {} waiting for you", if n == 1 { "session" } else { "sessions" }),
        Lang::Ru => format!("Ещё {}", tip_waiting(lang, n)),
    }
}

/// Gigabytes with one decimal: `24.2`, or `24,2` in Russian.
pub fn gb(lang: Lang, bytes: u64) -> String {
    let s = format!("{:.1}", bytes as f64 / GIB);
    match lang {
        Lang::En => s,
        Lang::Ru => s.replace('.', ","),
    }
}

fn unit(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "GB",
        Lang::Ru => "ГБ",
    }
}

/// One memory value with its unit: `6.2 GB`.
pub fn gb_value(lang: Lang, bytes: u64) -> String {
    format!("{} {}", gb(lang, bytes), unit(lang))
}

/// Used of total: `24.2 of 31.8 GB`.
pub fn mem_total(lang: Lang, used: u64, total: u64) -> String {
    let of = match lang {
        Lang::En => "of",
        Lang::Ru => "из",
    };
    format!("{} {of} {} {}", gb(lang, used), gb(lang, total), unit(lang))
}

pub fn tip_memory(lang: Lang, used: u64, total: u64) -> String {
    match lang {
        Lang::En => format!("{} in use", mem_total(lang, used, total)),
        Lang::Ru => format!("Занято {}", mem_total(lang, used, total)),
    }
}

/// How long a state has lasted: `now`, `7 min`, `3 h`, `2 d`.
pub fn ago(lang: Lang, ms: i64) -> String {
    let (now, min, h, d) = match lang {
        Lang::En => ("now", "min", "h", "d"),
        Lang::Ru => ("сейчас", "мин", "ч", "дн"),
    };
    match ms.max(0) {
        m if m < 60_000 => now.to_owned(),
        m if m < 3_600_000 => format!("{} {min}", m / 60_000),
        m if m < DAY_MS => format!("{} {h}", m / 3_600_000),
        m => format!("{} {d}", m / DAY_MS),
    }
}

/// `at 20:40` today, otherwise `Sun 18:00`.
pub fn reset_text(lang: Lang, at: i64, now: i64, tz_offset_min: i32) -> String {
    let offset = i64::from(tz_offset_min) * 60_000;
    let (local_at, local_now) = (at + offset, now + offset);
    let minutes = local_at.rem_euclid(DAY_MS) / 60_000;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    let (day_at, day_now) = (local_at.div_euclid(DAY_MS), local_now.div_euclid(DAY_MS));
    let day = day_at.rem_euclid(7) as usize;
    match (lang, day_at == day_now) {
        (Lang::En, true) => format!("at {clock}"),
        (Lang::Ru, true) => format!("в {clock}"),
        (Lang::En, false) => format!("{} {clock}", WEEKDAYS_EN[day]),
        (Lang::Ru, false) => format!("{} {clock}", WEEKDAYS_RU[day]),
    }
}

fn resets(lang: Lang, view: &LimitView, now: i64, tz: i32) -> String {
    let word = match lang {
        Lang::En => "resets",
        Lang::Ru => "сброс",
    };
    view.resets_at.map(|at| format!(", {word} {}", reset_text(lang, at, now, tz))).unwrap_or_default()
}

pub fn tip_limit(lang: Lang, view: &LimitView, now: i64, tz_offset_min: i32) -> String {
    let reset = resets(lang, view, now, tz_offset_min);
    match lang {
        Lang::En => {
            let window = match view.window {
                LimitWindow::FiveHour => "5-hour",
                LimitWindow::Week => "weekly",
            };
            format!("{}% of the {window} limit used{reset}", view.used)
        }
        Lang::Ru => {
            let window = match view.window {
                LimitWindow::FiveHour => "5-часового",
                LimitWindow::Week => "недельного",
            };
            format!("Израсходовано {}% {window} лимита{reset}", view.used)
        }
    }
}

/// The line under a limit ring in the sessions window: `week, resets Sun 18:00`.
pub fn limit_caption(lang: Lang, view: &LimitView, now: i64, tz_offset_min: i32) -> String {
    let window = match (lang, view.window) {
        (Lang::En, LimitWindow::FiveHour) => "5 hours",
        (Lang::En, LimitWindow::Week) => "week",
        (Lang::Ru, LimitWindow::FiveHour) => "5 часов",
        (Lang::Ru, LimitWindow::Week) => "неделя",
    };
    format!("{window}{}", resets(lang, view, now, tz_offset_min))
}

/// `48 total`.
pub fn total(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{n} total"),
        Lang::Ru => format!("{n} всего"),
    }
}

/// The folded rest of a group: `and 3 more`.
pub fn more(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("and {n} more"),
        Lang::Ru => format!("и ещё {n}"),
    }
}

/// A session's question in the language's own quotation marks.
pub fn quote(lang: Lang, s: &str) -> String {
    match lang {
        Lang::En => format!("\u{201c}{s}\u{201d}"),
        Lang::Ru => format!("«{s}»"),
    }
}

/// The local offset from UTC in minutes, daylight saving included.
pub fn local_tz_offset_min() -> i32 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut info = TIME_ZONE_INFORMATION::default();
    // SAFETY: the out pointer is a live, properly sized struct.
    let kind = unsafe { GetTimeZoneInformation(&mut info) };
    let daylight = if kind == 2 { info.DaylightBias } else { info.StandardBias };
    -(info.Bias + daylight)
}

/// The Plane board's words.
pub struct BoardWords {
    pub title: &'static str,
    pub all: &'static str,
    /// Task columns: not taken, in progress, done today.
    pub columns: [&'static str; 3],
    pub agents: &'static str,
    pub new_agent: &'static str,
    pub hint: &'static str,
    pub empty_column: &'static str,
    pub no_agents: &'static str,
    pub no_board: &'static str,
    pub updated: &'static str,
    pub refreshing: &'static str,
    pub sending: &'static str,
    pub moving: &'static str,
    pub drop_session: &'static str,
    pub drop_new: &'static str,
    pub drop_column: &'static str,
    pub cannot_drop: &'static str,
    pub timeout: &'static str,
    pub write_failed: &'static str,
    pub idle: &'static str,
    pub tip_tasks: &'static str,
    pub menu_board: &'static str,
}

const BOARD_EN: BoardWords = BoardWords {
    title: "Plane tasks",
    all: "All",
    columns: ["Not taken", "In progress", "Done and in review"],
    agents: "Agents",
    new_agent: "New agent",
    hint: "Drag a task onto an agent, into a new chat or to another column",
    empty_column: "Nothing here",
    no_agents: "No chats open",
    no_board: "No board yet: the status-band mod has not fetched Plane",
    updated: "updated",
    refreshing: "Asking Plane again…",
    sending: "sending…",
    moving: "moving…",
    drop_session: "Give to",
    drop_new: "New chat in",
    drop_column: "Move to",
    cannot_drop: "This chat has no app link: it cannot take a task",
    timeout: "No Claude session with the mod answered",
    write_failed: "Could not write the request",
    idle: "idle",
    tip_tasks: "Plane tasks",
    menu_board: "Task board",
};

const BOARD_RU: BoardWords = BoardWords {
    title: "Задачи Plane",
    all: "Все",
    columns: ["Не взято", "В работе", "Готово и на проверке"],
    agents: "Агенты",
    new_agent: "Новый агент",
    hint: "Перетащи задачу на агента, в новый чат или в другую колонку",
    empty_column: "Пусто",
    no_agents: "Открытых чатов нет",
    no_board: "Доски пока нет: мод status-band ещё не забрал Plane",
    updated: "обновлено",
    refreshing: "Запрашиваю Plane заново…",
    sending: "отправляю…",
    moving: "переношу…",
    drop_session: "Отдать",
    drop_new: "Новый чат в",
    drop_column: "Перенести в",
    cannot_drop: "У этого чата нет ссылки приложения: задачу ему не отдать",
    timeout: "Ни одна сессия Claude с модом не ответила",
    write_failed: "Не удалось записать заявку",
    idle: "тихо",
    tip_tasks: "Задачи Plane",
    menu_board: "Доска задач",
};

impl Lang {
    pub fn board(self) -> &'static BoardWords {
        match self {
            Lang::En => &BOARD_EN,
            Lang::Ru => &BOARD_RU,
        }
    }
}

/// «7 в работе, 50 не взято» for the strip's tooltip.
pub fn tip_tasks(lang: Lang, doing: usize, todo: usize) -> String {
    match lang {
        Lang::En => format!("Plane: {doing} in progress, {todo} not taken"),
        Lang::Ru => format!("Plane: {doing} в работе, {todo} не взято"),
    }
}

/// «обновлено 2 мин назад», «обновлено только что».
pub fn updated_ago(lang: Lang, ms: i64) -> String {
    match (lang, ms) {
        (Lang::En, m) if m < 60_000 => "updated just now".to_owned(),
        (Lang::Ru, m) if m < 60_000 => "обновлено только что".to_owned(),
        (Lang::En, m) => format!("updated {} ago", ago(lang, m)),
        (Lang::Ru, m) => format!("обновлено {} назад", ago(lang, m)),
    }
}

/// `2026-11-18 19:16:09`, local time, for the log.
pub fn local_stamp() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64) + i64::from(local_tz_offset_min()) * 60_000;
    let (days, rest) = (ms.div_euclid(DAY_MS), ms.rem_euclid(DAY_MS) / 1000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}
