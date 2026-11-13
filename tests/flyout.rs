use claudibeat::memory::MemGroups;
use claudibeat::merge::{Row, groups};
use claudibeat::model::SessionState;
use claudibeat::render::flyout::{FlyoutModel, layout_flyout};
use claudibeat::render::{Renderer, TRANSPARENT};

fn row(key: &str, state: SessionState, link: bool) -> Row {
    Row {
        key: key.into(),
        desktop_id: Some(key.into()),
        title: format!("Сессия {key}"),
        project: "demo".into(),
        link: link.then(|| format!("claude://claude.ai/epitaxy/{key}")),
        state,
        ask: Some("Какой вариант оставить?".into()),
        since: 0,
        doing: Some("запускает тесты".into()),
        needs_you: state != SessionState::Working,
        last_activity_at: 0,
    }
}

fn rows(waiting: usize) -> Vec<Row> {
    let mut v: Vec<Row> = (0..waiting).map(|i| row(&format!("w{i}"), SessionState::Waiting, true)).collect();
    v.push(row("d0", SessionState::Done, false));
    v.push(row("r0", SessionState::Working, true));
    v
}

fn model<'a>(rows: &'a [Row], expanded: [bool; 3]) -> FlyoutModel<'a> {
    FlyoutModel {
        groups: groups(rows),
        total: rows.len(),
        expanded,
        mem: MemGroups { claude: 6 << 30, chrome: 1 << 30, python: 1 << 29, other: 10 << 30, used: 18 << 30, total: 32 << 30 },
        limits: Vec::new(),
        now: 0,
        tz: 0,
    }
}

#[test]
fn collapsed_group_shows_three_and_more() {
    let r = Renderer::new().unwrap();
    let data = rows(5);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let l = layout_flyout(&model(&data, [false; 3]), p, 2_000.0);
        let waiting = l.rows.iter().filter(|(k, _, _)| k.starts_with('w')).count();
        assert_eq!(waiting, 3);
        assert_eq!(l.more.iter().map(|(g, _)| *g).collect::<Vec<_>>(), [0]);
        let expanded = layout_flyout(&model(&data, [true, false, false]), p, 2_000.0);
        assert_eq!(expanded.rows.iter().filter(|(k, _, _)| k.starts_with('w')).count(), 5);
        assert!(expanded.more.is_empty());
        assert!(expanded.height > l.height);
    })
    .unwrap();
}

#[test]
fn rows_without_link_are_not_clickable() {
    let r = Renderer::new().unwrap();
    let data = rows(1);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let l = layout_flyout(&model(&data, [false; 3]), p, 2_000.0);
        let clickable = |key: &str| l.rows.iter().find(|(k, _, _)| k == key).map(|(_, _, c)| *c);
        assert_eq!(clickable("w0"), Some(true));
        assert_eq!(clickable("d0"), Some(false));
    })
    .unwrap();
}

#[test]
fn tall_list_is_capped_and_scrolls() {
    let r = Renderer::new().unwrap();
    let data = rows(12);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let full = layout_flyout(&model(&data, [true, false, false]), p, 5_000.0);
        let capped = layout_flyout(&model(&data, [true, false, false]), p, 600.0);
        assert!(full.height > 600.0);
        assert_eq!(capped.height, 600.0);
        assert!(capped.scroll_max > 0.0);
        assert_eq!(full.scroll_max, 0.0);
    })
    .unwrap();
}
