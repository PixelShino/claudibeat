use claudibeat::limits::{LimitWindow, limit_views, strip_limit};
use claudibeat::model::{AccountFile, Limit};
use claudibeat::text::*;

const GB: u64 = 1 << 30;
const MSK: i32 = 180;

/// 2026-11-13 18:11:00 MSK in ms since the epoch.
const NOW: i64 = 1_794_582_660_000;

#[test]
fn plural_forms() {
    for (n, w) in [(1, "сессия"), (2, "сессии"), (5, "сессий"), (11, "сессий"), (12, "сессий"), (14, "сессий"), (21, "сессия"), (22, "сессии"), (0, "сессий"), (111, "сессий"), (101, "сессия")] {
        assert_eq!(plural(n, "сессия", "сессии", "сессий"), w, "n = {n}");
    }
}

#[test]
fn tips() {
    assert_eq!(tip_waiting(1), "1 сессия ждёт ответа");
    assert_eq!(tip_waiting(12), "12 сессий ждут ответа");
    assert_eq!(tip_done(1), "1 сессия закончила, не открыта");
    assert_eq!(tip_done(2), "2 сессии закончили, не открыты");
    assert_eq!(tip_done(21), "21 сессия закончила, не открыта");
    assert_eq!(tip_working(4), "4 сессии в работе");
    assert_eq!(tip_working(0), "0 сессий в работе");
}

#[test]
fn memory_tip() {
    assert_eq!(gb(26 * GB + GB / 5), "26,2");
    assert_eq!(tip_memory(242 * GB / 10, 318 * GB / 10), "Занято 24,2 из 31,8 ГБ");
}

#[test]
fn ago_steps() {
    assert_eq!(ago(30_000), "сейчас");
    assert_eq!(ago(7 * 60_000), "7 мин");
    assert_eq!(ago(3 * 3_600_000), "3 ч");
    assert_eq!(ago(2 * 86_400_000), "2 дн");
}

#[test]
fn reset_week_and_today() {
    // 2026-11-15 18:00 MSK, a Sunday.
    assert_eq!(reset_text(1_794_754_800_000, NOW, MSK), "вс 18:00");
    // Today 20:40 MSK.
    assert_eq!(reset_text(NOW + (2 * 60 + 29) * 60_000, NOW, MSK), "в 20:40");
}

fn account(updated_at: i64) -> AccountFile {
    AccountFile {
        v: 1,
        updated_at,
        limits: vec![
            Limit { window: "five_hour".into(), percent: 23.0, resets_at: Some(NOW + 9_000_000) },
            Limit { window: "seven_day".into(), percent: 66.0, resets_at: Some(1_794_754_800_000) },
            Limit { window: "seven_day_opus".into(), percent: 1.0, resets_at: None },
        ],
    }
}

#[test]
fn limits_pick_and_stale() {
    let fresh = account(NOW - 60_000);
    let views = limit_views(Some(&fresh), NOW);
    assert_eq!(views.len(), 2, "unknown windows are skipped");
    let shown = strip_limit(&views).unwrap();
    assert_eq!(shown.window, LimitWindow::Week);
    assert_eq!((shown.used, shown.remaining), (66, 34));
    assert!((shown.used_fraction() - 0.66).abs() < 1e-6, "the ring fills with what is used");
    assert!(limit_views(Some(&account(NOW - 7 * 3_600_000)), NOW).is_empty());
    assert!(limit_views(None, NOW).is_empty());
}

#[test]
fn limit_tip() {
    let views = limit_views(Some(&account(NOW)), NOW);
    let week = views.iter().find(|v| v.window == LimitWindow::Week).unwrap();
    assert_eq!(tip_limit(week, NOW, MSK), "Израсходовано 66% недельного лимита, сброс вс 18:00");
    let five = views.iter().find(|v| v.window == LimitWindow::FiveHour).unwrap();
    assert_eq!(tip_limit(five, NOW, MSK), "Израсходовано 23% 5-часового лимита, сброс в 20:41");
}

#[test]
#[ignore = "depends on this machine's time zone"]
fn live_tz_print() {
    println!("TZ {}", local_tz_offset_min());
}

#[test]
fn toast_titles() {
    assert_eq!(title_done_batch(3), "3 сессии закончили");
    assert_eq!(title_done_batch(21), "21 сессия закончила");
    assert_eq!(title_more_waiting(2), "Ещё 2 сессии ждут ответа");
    assert_eq!(title_more_waiting(5), "Ещё 5 сессий ждут ответа");
}
