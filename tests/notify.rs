use claudibeat::merge::Row;
use claudibeat::model::SessionState;
use claudibeat::notify::{Notifier, ToastEvent, ToastKind, ToastStack};
use std::collections::HashSet;

const T: i64 = 1_000_000;

fn row(key: &str, state: SessionState, needs_you: bool) -> Row {
    Row {
        key: key.into(),
        desktop_id: Some(key.into()),
        title: key.into(),
        project: "p".into(),
        link: None,
        state,
        ask: None,
        since: 0,
        doing: None,
        needs_you,
        last_activity_at: 0,
    }
}

fn waiting(key: &str) -> Row {
    row(key, SessionState::Waiting, true)
}
fn done(key: &str) -> Row {
    row(key, SessionState::Done, true)
}
fn seen(key: &str) -> Row {
    row(key, SessionState::Idle, false)
}

fn kinds(events: &[ToastEvent]) -> Vec<(ToastKind, Vec<String>)> {
    events.iter().map(|e| (e.kind, e.keys.clone())).collect()
}

#[test]
fn first_observe_is_silent() {
    let mut n = Notifier::new();
    assert!(n.observe(&[waiting("a"), done("b")], T).is_empty());
    assert!(n.observe(&[waiting("a"), done("b")], T + 5_000).is_empty(), "nothing changed, nothing to say");
}

#[test]
fn new_waiting_fires_immediately() {
    let mut n = Notifier::new();
    n.observe(&[seen("a")], T);
    assert_eq!(kinds(&n.observe(&[waiting("a")], T + 2_000)), [(ToastKind::Waiting, vec!["a".to_string()])]);
}

#[test]
fn throttle_10s_per_key() {
    let mut n = Notifier::new();
    n.observe(&[seen("a")], T);
    assert_eq!(n.observe(&[waiting("a")], T + 1_000).len(), 1);
    n.observe(&[seen("a")], T + 2_000);
    assert!(n.observe(&[waiting("a")], T + 6_000).is_empty(), "within 10 s of the last toast");
    n.observe(&[seen("a")], T + 8_000);
    assert_eq!(n.observe(&[waiting("a")], T + 12_000).len(), 1);
}

#[test]
fn opened_suppresses_5s() {
    let mut n = Notifier::new();
    n.observe(&[seen("a")], T);
    n.opened("a", T + 1_000);
    assert!(n.observe(&[waiting("a")], T + 4_000).is_empty());
    n.observe(&[seen("a")], T + 5_000);
    assert_eq!(n.observe(&[waiting("a")], T + 7_000).len(), 1);
}

#[test]
fn done_batched_within_2s() {
    let mut n = Notifier::new();
    n.observe(&[seen("a"), seen("b"), seen("c")], T);
    assert!(n.observe(&[done("a"), seen("b"), seen("c")], T + 100).is_empty());
    assert!(n.observe(&[done("a"), done("b"), seen("c")], T + 900).is_empty());
    assert!(n.observe(&[done("a"), done("b"), done("c")], T + 1_600).is_empty());
    let out = n.observe(&[done("a"), done("b"), done("c")], T + 2_100);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind, ToastKind::DoneBatch);
    assert_eq!(out[0].count, 3);
}

#[test]
fn single_done_after_2s() {
    let mut n = Notifier::new();
    n.observe(&[seen("a")], T);
    assert!(n.observe(&[done("a")], T + 100).is_empty());
    assert_eq!(kinds(&n.observe(&[done("a")], T + 2_200)), [(ToastKind::Done, vec!["a".to_string()])]);
}

#[test]
fn done_seen_before_flush_is_dropped() {
    let mut n = Notifier::new();
    n.observe(&[seen("a")], T);
    n.observe(&[done("a")], T + 100);
    assert!(n.observe(&[seen("a")], T + 2_500).is_empty());
}

fn ev(kind: ToastKind, key: &str, at: i64) -> ToastEvent {
    ToastEvent { kind, keys: vec![key.into()], at, count: 1 }
}

#[test]
fn stack_evicts_oldest_done() {
    let mut s = ToastStack::default();
    s.push(ev(ToastKind::Done, "d", 1));
    s.push(ev(ToastKind::Waiting, "w1", 2));
    s.push(ev(ToastKind::Waiting, "w2", 3));
    s.push(ev(ToastKind::Waiting, "w3", 4));
    let keys: Vec<_> = s.items.iter().map(|e| e.keys[0].as_str()).collect();
    assert_eq!(keys, ["w1", "w2", "w3"]);
}

#[test]
fn stack_full_of_waiting_turns_summary() {
    let mut s = ToastStack::default();
    for (i, k) in ["w1", "w2", "w3", "w4"].iter().enumerate() {
        s.push(ev(ToastKind::Waiting, k, i as i64));
    }
    assert_eq!(s.items.len(), 3);
    assert_eq!(s.items[2].kind, ToastKind::MoreWaiting);
    assert_eq!(s.items[2].count, 2);
    s.push(ev(ToastKind::Waiting, "w5", 9));
    assert_eq!(s.items[2].count, 3);
    s.push(ev(ToastKind::Done, "d", 10));
    assert_eq!(s.items.len(), 3, "a finished session never pushes out someone waiting");
}

#[test]
fn stack_forgets_sessions_seen_elsewhere() {
    let mut s = ToastStack::default();
    s.push(ev(ToastKind::Waiting, "w1", 1));
    s.push(ev(ToastKind::Done, "d1", 2));
    let needing: HashSet<String> = ["w1".to_string()].into();
    s.retain_needing(&needing);
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].keys, ["w1"]);
}
