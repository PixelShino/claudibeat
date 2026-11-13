use claudibeat::merge::{IDLE_KEEP_MS, STALE_MS, counts, groups, merge_rows, open_target, project_of};
use claudibeat::model::{AckFile, IndexEntry, SessionFile, SessionState};
use std::collections::HashMap;

const NOW: i64 = 1_000_000_000_000;

fn entry(id: &str, running: bool, last: i64) -> IndexEntry {
    IndexEntry {
        desktop_id: id.into(),
        title: format!("T {id}"),
        cwd: format!("D:\\Code\\Projects\\{id}"),
        is_running: running,
        last_activity_at: last,
        link: Some(format!("claude://claude.ai/epitaxy/{id}")),
    }
}

fn file(key: &str, desktop: Option<&str>, state: SessionState, since: i64, beat: i64) -> SessionFile {
    SessionFile {
        v: 1,
        key: key.into(),
        desktop_id: desktop.map(Into::into),
        title: Some(format!("F {key}")),
        cwd: "D:\\x".into(),
        link: None,
        state,
        ask: Some("вопрос?".into()),
        state_since: since,
        doing: Some("делает".into()),
        heartbeat_at: beat,
    }
}

fn ack(at: i64) -> AckFile {
    AckFile { v: 1, ack_at: at, by: "t".into() }
}

fn no_acks() -> HashMap<String, AckFile> {
    HashMap::new()
}

#[test]
fn live_file_speaks() {
    let rows = merge_rows(
        &[entry("local_a", false, NOW - 5_000)],
        &[file("local_a", Some("local_a"), SessionState::Waiting, NOW - 2_000, NOW - 1_000)],
        &no_acks(),
        NOW,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, SessionState::Waiting);
    assert!(rows[0].needs_you);
    assert_eq!(rows[0].ask.as_deref(), Some("вопрос?"));
    assert_eq!(rows[0].title, "T local_a");
}

#[test]
fn stale_file_follows_index_running() {
    let rows = merge_rows(
        &[entry("local_a", true, NOW - 5_000)],
        &[file("local_a", Some("local_a"), SessionState::Idle, NOW - 9_000_000, NOW - STALE_MS - 1)],
        &no_acks(),
        NOW,
    );
    assert_eq!(rows[0].state, SessionState::Working);
    assert_eq!(rows[0].doing, None, "a quiet file does not say what it does");
}

#[test]
fn stale_waiting_keeps_word_when_app_knows_it() {
    let since = NOW - 7_200_000;
    let rows = merge_rows(
        &[entry("local_a", false, NOW - 3_600_000)],
        &[file("local_a", Some("local_a"), SessionState::Done, since, NOW - STALE_MS - 1)],
        &no_acks(),
        NOW,
    );
    assert_eq!(rows[0].state, SessionState::Done);
    assert_eq!(rows[0].since, since);
    assert!(rows[0].needs_you);
}

#[test]
fn stale_loose_cli_file_goes_idle() {
    let old = NOW - IDLE_KEEP_MS - 1;
    let rows = merge_rows(&[], &[file("cli-1", None, SessionState::Waiting, old, old)], &no_acks(), NOW);
    assert!(rows.is_empty(), "an old quiet terminal session leaves the list");
    let recent = NOW - STALE_MS - 1;
    let rows = merge_rows(&[], &[file("cli-1", None, SessionState::Waiting, recent, recent)], &no_acks(), NOW);
    assert_eq!(rows[0].state, SessionState::Idle);
    assert!(!rows[0].needs_you);
}

#[test]
fn ack_after_since_silences() {
    let since = NOW - 10_000;
    let f = file("local_a", Some("local_a"), SessionState::Done, since, NOW - 1_000);
    let idx = [entry("local_a", false, NOW - 1_000)];
    let mut acks = HashMap::new();
    acks.insert("local_a".to_string(), ack(since));
    assert!(!merge_rows(&idx, std::slice::from_ref(&f), &acks, NOW)[0].needs_you);
    acks.insert("local_a".to_string(), ack(since - 1));
    assert!(merge_rows(&idx, &[f], &acks, NOW)[0].needs_you);
}

#[test]
fn ack_by_desktop_id() {
    let since = NOW - 10_000;
    let mut acks = HashMap::new();
    acks.insert("local_a".to_string(), ack(since + 1));
    let rows = merge_rows(
        &[entry("local_a", false, NOW - 1_000)],
        &[file("cli-7", Some("local_a"), SessionState::Waiting, since, NOW - 1_000)],
        &acks,
        NOW,
    );
    assert_eq!(rows[0].key, "cli-7");
    assert!(!rows[0].needs_you);
}

#[test]
fn file_without_index_row_kept() {
    let rows = merge_rows(&[], &[file("local_new", Some("local_new"), SessionState::Working, NOW - 100, NOW - 100)], &no_acks(), NOW);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].desktop_id.as_deref(), Some("local_new"));
    assert_eq!(rows[0].title, "F local_new");
}

#[test]
fn newest_file_per_desktop_wins() {
    let rows = merge_rows(
        &[entry("local_a", false, NOW - 9_000)],
        &[
            file("cli-old", Some("local_a"), SessionState::Idle, NOW - 9_000, NOW - 5_000),
            file("local_a", Some("local_a"), SessionState::Working, NOW - 2_000, NOW - 1_000),
        ],
        &no_acks(),
        NOW,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, SessionState::Working);
    assert_eq!(rows[0].key, "local_a");
}

#[test]
fn idle_older_than_day_dropped() {
    let rows = merge_rows(&[entry("local_a", false, NOW - IDLE_KEEP_MS - 1), entry("local_b", false, NOW - 1_000)], &[], &no_acks(), NOW);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "local_b");
}

#[test]
fn counts_and_groups() {
    let mut acks = HashMap::new();
    acks.insert("d1".to_string(), ack(NOW));
    let files = [
        file("w1", Some("w1"), SessionState::Waiting, NOW - 50_000, NOW - 1_000),
        file("w2", Some("w2"), SessionState::Waiting, NOW - 90_000, NOW - 1_000),
        file("d1", Some("d1"), SessionState::Done, NOW - 10_000, NOW - 1_000),
        file("r1", Some("r1"), SessionState::Working, NOW - 10_000, NOW - 3_000),
        file("r2", Some("r2"), SessionState::Working, NOW - 10_000, NOW - 1_000),
        file("r3", Some("r3"), SessionState::Working, NOW - 10_000, NOW - 2_000),
    ];
    let rows = merge_rows(&[], &files, &acks, NOW);
    let c = counts(&rows);
    assert_eq!((c.waiting, c.done, c.working, c.total), (2, 0, 3, 6));
    let g = groups(&rows);
    assert_eq!(g.waiting.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["w2", "w1"], "longest waiting first");
    assert!(g.done.is_empty(), "a seen session is not in «готово»");
    assert_eq!(g.working.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["r2", "r3", "r1"], "freshest first");
}

#[test]
fn open_target_only_claude_links() {
    let mut rows = merge_rows(&[entry("local_a", true, NOW)], &[], &no_acks(), NOW);
    assert_eq!(open_target(&rows[0]), Some("claude://claude.ai/epitaxy/local_a"));
    rows[0].link = None;
    assert_eq!(open_target(&rows[0]), None);
    rows[0].link = Some("https://evil.example".into());
    assert_eq!(open_target(&rows[0]), None);
}

#[test]
fn project_names() {
    assert_eq!(project_of("D:\\Code\\Projects\\coffee-site"), "coffee-site");
    assert_eq!(project_of("C:/Users/x/scratch-workspaces/1/2"), "", "no folder: the window names it in its own language");
    assert_eq!(project_of(""), "");
}

#[test]
#[ignore = "reads the live status-band folder"]
fn live_counts_print() {
    use claudibeat::store::Store;
    let s = Store::new(Store::default_dir()).read();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let rows = merge_rows(&s.index, &s.files, &s.acks, now);
    println!("LIVE {:?}", counts(&rows));
}
