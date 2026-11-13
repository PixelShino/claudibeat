//! Every word claudibeat shows, in Russian, with the right plural forms.

use crate::limits::{LimitView, LimitWindow};

const GIB: f64 = (1u64 << 30) as f64;
const DAY_MS: i64 = 86_400_000;
const WEEKDAYS: [&str; 7] = ["чт", "пт", "сб", "вс", "пн", "вт", "ср"]; // 1970-01-01 was a Thursday

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

fn sessions(n: usize) -> String {
    format!("{n} {}", plural(n, "сессия", "сессии", "сессий"))
}

pub fn tip_waiting(n: usize) -> String {
    format!("{} {} ответа", sessions(n), plural(n, "ждёт", "ждут", "ждут"))
}

pub fn tip_done(n: usize) -> String {
    let verb = plural(n, "закончила", "закончили", "закончили");
    let open = plural(n, "открыта", "открыты", "открыты");
    format!("{} {verb}, не {open}", sessions(n))
}

pub fn tip_working(n: usize) -> String {
    format!("{} в работе", sessions(n))
}

/// Gigabytes with one decimal and a decimal comma: `24,2`.
pub fn gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / GIB).replace('.', ",")
}

pub fn tip_memory(used: u64, total: u64) -> String {
    format!("Занято {} из {} ГБ", gb(used), gb(total))
}

/// How long a state has lasted: «сейчас», «7 мин», «3 ч», «2 дн».
pub fn ago(ms: i64) -> String {
    match ms.max(0) {
        m if m < 60_000 => "сейчас".to_owned(),
        m if m < 3_600_000 => format!("{} мин", m / 60_000),
        m if m < DAY_MS => format!("{} ч", m / 3_600_000),
        m => format!("{} дн", m / DAY_MS),
    }
}

/// «в 20:40» today, otherwise «вс 18:00».
pub fn reset_text(at: i64, now: i64, tz_offset_min: i32) -> String {
    let offset = i64::from(tz_offset_min) * 60_000;
    let (local_at, local_now) = (at + offset, now + offset);
    let minutes = local_at.rem_euclid(DAY_MS) / 60_000;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    let (day_at, day_now) = (local_at.div_euclid(DAY_MS), local_now.div_euclid(DAY_MS));
    if day_at == day_now {
        format!("в {clock}")
    } else {
        format!("{} {clock}", WEEKDAYS[day_at.rem_euclid(7) as usize])
    }
}

pub fn tip_limit(view: &LimitView, now: i64, tz_offset_min: i32) -> String {
    let window = match view.window {
        LimitWindow::FiveHour => "5-часового",
        LimitWindow::Week => "недельного",
    };
    let reset = view.resets_at.map(|at| format!(", сброс {}", reset_text(at, now, tz_offset_min))).unwrap_or_default();
    format!("Осталось {}% {window} лимита{reset}", view.remaining)
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
