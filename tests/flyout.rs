use claudibeat::memory::MemGroups;
use claudibeat::merge::{Row, groups};
use claudibeat::model::SessionState;
use claudibeat::render::flyout::{FlyoutHit, FlyoutLayout, FlyoutModel, FlyoutView, Prefs, layout_flyout};
use claudibeat::render::{Renderer, TRANSPARENT};
use claudibeat::text::Lang;

fn row(key: &str, state: SessionState, link: bool) -> Row {
    Row {
        key: key.into(),
        desktop_id: Some(key.into()),
        title: format!("Сессия {key}"),
        project: "demo".into(),
        cwd: String::new(),
        link: link.then(|| format!("claude://claude.ai/epitaxy/{key}")),
        state,
        ask: Some("Какой вариант оставить?".into()),
        since: 0,
        doing: Some("запускает тесты".into()),
        needs_you: state != SessionState::Working,
        last_activity_at: 0,
        parent: None,
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
        lang: Lang::En,
        view: FlyoutView::Sessions,
        prefs: Prefs::default(),
    }
}

fn control(l: &FlyoutLayout, hit: &FlyoutHit) -> claudibeat::render::Rect {
    l.controls.iter().find(|(h, _)| h == hit).map(|(_, r)| *r).unwrap_or_else(|| panic!("no {hit:?} in {:?}", l.controls))
}

fn centre(r: claudibeat::render::Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
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

#[test]
fn the_gear_sits_in_the_header_and_answers_clicks() {
    let r = Renderer::new().unwrap();
    let data = rows(2);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let l = layout_flyout(&model(&data, [false; 3]), p, 2_000.0);
        let gear = control(&l, &FlyoutHit::Gear);
        assert!(gear.bottom() <= l.list.y, "the gear belongs to the header, above the list");
        assert!(gear.right() <= l.card.right() && gear.x > l.card.x + l.card.w / 2.0, "on the right");
        let (x, y) = centre(gear);
        assert_eq!(l.hit(x, y, 0.0), Some(FlyoutHit::Gear));
    })
    .unwrap();
}

#[test]
fn the_footer_holds_do_not_disturb_and_the_language() {
    let r = Renderer::new().unwrap();
    let data = rows(2);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let l = layout_flyout(&model(&data, [false; 3]), p, 2_000.0);
        for hit in [FlyoutHit::Dnd, FlyoutHit::Lang(Lang::En), FlyoutHit::Lang(Lang::Ru)] {
            let rect = control(&l, &hit);
            assert!(rect.y >= l.list.bottom(), "{hit:?} sits below the sessions");
            assert!(rect.bottom() <= l.card.bottom());
            let (x, y) = centre(rect);
            assert_eq!(l.hit(x, y, 0.0), Some(hit.clone()));
            assert_eq!(l.hit(x, y, 30.0), Some(hit), "controls do not scroll with the list");
        }
        let dnd = control(&l, &FlyoutHit::Dnd);
        let en = control(&l, &FlyoutHit::Lang(Lang::En));
        assert!(dnd.right() < en.x, "do not disturb on the left, the language on the right");
    })
    .unwrap();
}

#[test]
fn settings_replace_the_list_and_lead_back() {
    let r = Renderer::new().unwrap();
    let data = rows(2);
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| {
        let mut m = model(&data, [false; 3]);
        m.view = FlyoutView::Settings;
        let l = layout_flyout(&m, p, 2_000.0);
        assert!(l.rows.is_empty(), "no sessions on the settings page");
        let back = control(&l, &FlyoutHit::Back);
        assert!(back.x < l.card.x + 40.0, "back sits at the top left");
        let order: Vec<f32> = [FlyoutHit::Dnd, FlyoutHit::Sound, FlyoutHit::Autostart, FlyoutHit::Lang(Lang::En)]
            .iter()
            .map(|h| control(&l, h).y)
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "one setting per row, top to bottom: {order:?}");
        let (x, y) = centre(control(&l, &FlyoutHit::Lang(Lang::Ru)));
        assert_eq!(l.hit(x, y, 0.0), Some(FlyoutHit::Lang(Lang::Ru)));
    })
    .unwrap();
}
