use claudibeat::merge::Row;
use claudibeat::model::SessionState;
use claudibeat::notify::{ToastEvent, ToastKind};
use claudibeat::render::toast::{layout_toast, toast_model};
use claudibeat::render::{Renderer, TRANSPARENT};
use claudibeat::text::Lang;

fn row(key: &str, link: bool) -> Row {
    Row {
        key: key.into(),
        desktop_id: None,
        title: format!("Сессия {key}"),
        project: "demo".into(),
        link: link.then(|| format!("claude://claude.ai/epitaxy/{key}")),
        state: SessionState::Waiting,
        ask: Some("Оставить старый API?".into()),
        since: 0,
        doing: None,
        needs_you: true,
        last_activity_at: 0,
    }
}

fn ev(kind: ToastKind, keys: &[&str]) -> ToastEvent {
    ToastEvent { kind, keys: keys.iter().map(|k| k.to_string()).collect(), at: 0, count: keys.len() }
}

#[test]
fn waiting_toast_quotes_the_question_and_opens_chat() {
    let rows = [row("a", true)];
    let m = toast_model(&ev(ToastKind::Waiting, &["a"]), &rows, 0, Lang::Ru);
    assert_eq!(m.label, "Ждёт ответа");
    assert_eq!(m.title, "Сессия a");
    assert_eq!(m.sub.as_deref(), Some("«Оставить старый API?»"));
    assert_eq!(m.action, Some("Открыть чат"));
}

#[test]
fn no_link_no_action() {
    let rows = [row("a", false)];
    let m = toast_model(&ev(ToastKind::Done, &["a"]), &rows, 0, Lang::Ru);
    assert_eq!(m.action, None);
    let r = Renderer::new().unwrap();
    r.render(10.0, 10.0, 1.0, TRANSPARENT, |p| assert!(layout_toast(&m, p).action.is_none())).unwrap();
}

#[test]
fn batch_and_summary_titles() {
    let rows = [row("a", true), row("b", true), row("c", true)];
    let batch = toast_model(&ev(ToastKind::DoneBatch, &["a", "b", "c"]), &rows, 0, Lang::Ru);
    assert_eq!((batch.label.as_str(), batch.title.as_str()), ("Готово", "3 сессии закончили"));
    assert_eq!(batch.action, Some("Показать все"));
    let more = toast_model(&ev(ToastKind::MoreWaiting, &["a", "b"]), &rows, 0, Lang::Ru);
    assert_eq!(more.title, "Ещё 2 сессии ждут ответа");
}

#[test]
fn english_toast_words() {
    let rows = [row("a", true)];
    let m = toast_model(&ev(ToastKind::Waiting, &["a"]), &rows, 0, Lang::En);
    assert_eq!(m.label, "Waiting for you");
    assert_eq!(m.sub.as_deref(), Some("\u{201c}Оставить старый API?\u{201d}"));
    assert_eq!(m.action, Some("Open chat"));
    assert_eq!(m.age, "now");
    let batch = toast_model(&ev(ToastKind::DoneBatch, &["a", "b"]), &rows, 0, Lang::En);
    assert_eq!(batch.label, "Done");
    assert_eq!(batch.title, "2 sessions finished");
    assert_eq!(batch.action, Some("Show all"));
}
