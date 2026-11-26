use claudibeat::focus::{FocusReader, current};
use claudibeat::merge::{Row, seen_in_claude};
use claudibeat::model::SessionState;
use std::collections::HashMap;
use std::path::Path;

fn row(key: &str, state: SessionState, since: i64, needs_you: bool) -> Row {
    Row {
        key: key.into(),
        desktop_id: Some(key.into()),
        title: key.into(),
        project: String::new(),
        cwd: String::new(),
        link: None,
        state,
        ask: None,
        since,
        doing: None,
        needs_you,
        last_activity_at: since,
        parent: None,
    }
}

#[test]
fn a_chat_opened_after_it_finished_is_seen() {
    let rows = vec![
        row("a", SessionState::Done, 100, true),    // opened at 150: seen
        row("b", SessionState::Done, 100, true),    // opened at 90, before it finished: not seen
        row("c", SessionState::Waiting, 100, true), // never opened
        row("d", SessionState::Working, 100, false),
    ];
    let focus = HashMap::from([("a".to_owned(), 150), ("b".to_owned(), 90), ("d".to_owned(), 500)]);
    assert_eq!(seen_in_claude(&rows, &focus, None), ["a"]);
    // The chat in front right now counts as seen even before its focus stamp moves.
    assert_eq!(seen_in_claude(&rows, &focus, Some("c")), ["a", "c"]);
}

#[test]
fn the_chat_in_front_is_the_last_opened_and_only_with_claude_in_front() {
    let focus = HashMap::from([("a".to_owned(), 150), ("b".to_owned(), 900)]);
    assert_eq!(current(&focus, true), Some("b"));
    assert_eq!(current(&focus, false), None);
}

#[test]
fn focus_comes_from_claudes_own_session_files() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("focus_sessions");
    let _ = std::fs::remove_dir_all(&root);
    let org = root.join("account").join("org");
    std::fs::create_dir_all(&org).unwrap();
    std::fs::write(org.join("local_a.json"), r#"{"sessionId":"local_a","lastFocusedAt":1700,"title":"x","more":{"deep":[1,2]}}"#).unwrap();
    std::fs::write(org.join("local_b.json"), r#"{"sessionId":"local_b"}"#).unwrap();
    std::fs::write(org.join("other.json"), r#"{"sessionId":"nope","lastFocusedAt":5}"#).unwrap();
    let mut r = FocusReader::with_roots(vec![root.clone(), root.join("missing")]);
    assert_eq!(r.read(), HashMap::from([("local_a".to_owned(), 1700)]));
    std::fs::write(org.join("local_b.json"), r#"{"sessionId":"local_b","lastFocusedAt":1800}"#).unwrap();
    assert_eq!(r.read().get("local_b"), Some(&1800), "a changed file is read again");
}
