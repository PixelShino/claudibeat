use claudibeat::limits::{LimitWindow, limit_views, strip_limit};
use claudibeat::model::{AccountFile, Limit};
use claudibeat::text::*;

const GB: u64 = 1 << 30;
const MSK: i32 = 180;
const RU: Lang = Lang::Ru;
const EN: Lang = Lang::En;

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
    assert_eq!(tip_waiting(RU, 1), "1 сессия ждёт ответа");
    assert_eq!(tip_waiting(RU, 12), "12 сессий ждут ответа");
    assert_eq!(tip_done(RU, 1), "1 сессия закончила, не открыта");
    assert_eq!(tip_done(RU, 2), "2 сессии закончили, не открыты");
    assert_eq!(tip_done(RU, 21), "21 сессия закончила, не открыта");
    assert_eq!(tip_working(RU, 4), "4 сессии в работе");
    assert_eq!(tip_working(RU, 0), "0 сессий в работе");
}

#[test]
fn memory_tip() {
    assert_eq!(gb(RU, 26 * GB + GB / 5), "26,2");
    assert_eq!(tip_memory(RU, 242 * GB / 10, 318 * GB / 10), "Занято 24,2 из 31,8 ГБ");
}

#[test]
fn ago_steps() {
    assert_eq!(ago(RU, 30_000), "сейчас");
    assert_eq!(ago(RU, 7 * 60_000), "7 мин");
    assert_eq!(ago(RU, 3 * 3_600_000), "3 ч");
    assert_eq!(ago(RU, 2 * 86_400_000), "2 дн");
}

#[test]
fn reset_week_and_today() {
    // 2026-11-15 18:00 MSK, a Sunday.
    assert_eq!(reset_text(RU, 1_794_754_800_000, NOW, MSK), "вс 18:00");
    // Today 20:40 MSK.
    assert_eq!(reset_text(RU, NOW + (2 * 60 + 29) * 60_000, NOW, MSK), "в 20:40");
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
    assert_eq!(tip_limit(RU, week, NOW, MSK), "Израсходовано 66% недельного лимита, сброс вс 18:00");
    let five = views.iter().find(|v| v.window == LimitWindow::FiveHour).unwrap();
    assert_eq!(tip_limit(RU, five, NOW, MSK), "Израсходовано 23% 5-часового лимита, сброс в 20:41");
}

#[test]
#[ignore = "depends on this machine's time zone"]
fn live_tz_print() {
    println!("TZ {}", local_tz_offset_min());
}

#[test]
fn toast_titles() {
    assert_eq!(title_done_batch(RU, 3), "3 сессии закончили");
    assert_eq!(title_done_batch(RU, 21), "21 сессия закончила");
    assert_eq!(title_more_waiting(RU, 2), "Ещё 2 сессии ждут ответа");
    assert_eq!(title_more_waiting(RU, 5), "Ещё 5 сессий ждут ответа");
}

#[test]
fn english_is_the_default() {
    assert_eq!(Lang::default(), EN);
}

#[test]
fn english_tips() {
    assert_eq!(tip_waiting(EN, 1), "1 session waiting for you");
    assert_eq!(tip_waiting(EN, 12), "12 sessions waiting for you");
    assert_eq!(tip_done(EN, 1), "1 session finished, not opened yet");
    assert_eq!(tip_done(EN, 2), "2 sessions finished, not opened yet");
    assert_eq!(tip_working(EN, 4), "4 sessions working");
    assert_eq!(tip_working(EN, 0), "0 sessions working");
    assert_eq!(title_done_batch(EN, 3), "3 sessions finished");
    assert_eq!(title_more_waiting(EN, 1), "1 more session waiting for you");
    assert_eq!(title_more_waiting(EN, 2), "2 more sessions waiting for you");
}

#[test]
fn english_numbers_and_times() {
    assert_eq!(gb(EN, 26 * GB + GB / 5), "26.2");
    assert_eq!(tip_memory(EN, 242 * GB / 10, 318 * GB / 10), "24.2 of 31.8 GB in use");
    assert_eq!(mem_total(EN, 242 * GB / 10, 318 * GB / 10), "24.2 of 31.8 GB");
    assert_eq!(mem_total(RU, 242 * GB / 10, 318 * GB / 10), "24,2 из 31,8 ГБ");
    assert_eq!(gb_value(EN, 62 * GB / 10), "6.2 GB");
    assert_eq!(gb_value(RU, 62 * GB / 10), "6,2 ГБ");
    assert_eq!(ago(EN, 30_000), "now");
    assert_eq!(ago(EN, 7 * 60_000), "7 min");
    assert_eq!(ago(EN, 3 * 3_600_000), "3 h");
    assert_eq!(ago(EN, 2 * 86_400_000), "2 d");
    assert_eq!(reset_text(EN, 1_794_754_800_000, NOW, MSK), "Sun 18:00");
    assert_eq!(reset_text(EN, NOW + (2 * 60 + 29) * 60_000, NOW, MSK), "at 20:40");
    assert_eq!(total(EN, 48), "48 total");
    assert_eq!(total(RU, 48), "48 всего");
    assert_eq!(more(EN, 3), "and 3 more");
    assert_eq!(more(RU, 3), "и ещё 3");
    assert_eq!(quote(EN, "Keep it?"), "\u{201c}Keep it?\u{201d}");
    assert_eq!(quote(RU, "Оставить?"), "«Оставить?»");
}

#[test]
fn english_limits() {
    let views = limit_views(Some(&account(NOW)), NOW);
    let week = views.iter().find(|v| v.window == LimitWindow::Week).unwrap();
    assert_eq!(tip_limit(EN, week, NOW, MSK), "66% of the weekly limit used, resets Sun 18:00");
    assert_eq!(limit_caption(EN, week, NOW, MSK), "week, resets Sun 18:00");
    assert_eq!(limit_caption(RU, week, NOW, MSK), "неделя, сброс вс 18:00");
    let five = views.iter().find(|v| v.window == LimitWindow::FiveHour).unwrap();
    assert_eq!(tip_limit(EN, five, NOW, MSK), "23% of the 5-hour limit used, resets at 20:41");
    assert_eq!(limit_caption(EN, five, NOW, MSK), "5 hours, resets at 20:41");
}

#[test]
fn every_word_has_both_languages() {
    for lang in [EN, RU] {
        let w = lang.words();
        for s in [w.flyout_title, w.empty, w.memory, w.limits, w.open_chat, w.show_all, w.no_folder, w.menu_seen, w.menu_sound, w.menu_autostart, w.menu_language, w.menu_quit, w.limit_unknown] {
            assert!(!s.is_empty());
        }
    }
    assert_ne!(EN.words().flyout_title, RU.words().flyout_title);
}

#[test]
fn a_window_past_its_reset_reads_empty() {
    let account = AccountFile {
        v: 1,
        updated_at: NOW - 3_600_000,
        limits: vec![
            Limit { window: "five_hour".into(), percent: 90.0, resets_at: Some(NOW - 60_000) },
            Limit { window: "seven_day".into(), percent: 40.0, resets_at: Some(NOW + 86_400_000) },
        ],
    };
    let views = limit_views(Some(&account), NOW);
    let five = views.iter().find(|v| v.window == LimitWindow::FiveHour).unwrap();
    assert_eq!((five.used, five.remaining, five.resets_at), (0, 100, None), "the window has started over");
    assert_eq!(strip_limit(&views).unwrap().window, LimitWindow::Week, "the strip shows the live window");
}
