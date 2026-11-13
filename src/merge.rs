//! The sessions as the person sees them.
//!
//! A port of `mergeRows`/`rowOf` from the status-band mod
//! (`~/.claude/mods/status-band/hooks/shared.ts`), so the counts here always
//! match the band inside Claude. claudibeat is not a session, so there is no
//! "self" row.

use crate::model::{AckFile, IndexEntry, SessionFile, SessionState};
use crate::store::safe_key;
use std::collections::HashMap;

/// A file whose heartbeat is older than this no longer speaks for its session.
pub const STALE_MS: i64 = 90_000;
/// Idle sessions quieter than this leave the list.
pub const IDLE_KEEP_MS: i64 = 86_400_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub desktop_id: Option<String>,
    pub title: String,
    pub project: String,
    pub link: Option<String>,
    pub state: SessionState,
    pub ask: Option<String>,
    pub since: i64,
    pub doing: Option<String>,
    pub needs_you: bool,
    pub last_activity_at: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub waiting: usize,
    pub done: usize,
    pub working: usize,
    pub total: usize,
}

pub struct Groups<'a> {
    pub waiting: Vec<&'a Row>,
    pub done: Vec<&'a Row>,
    pub working: Vec<&'a Row>,
}

/// `D:\Code\Projects\coffee-site` → `coffee-site`. The app's own scratch folders
/// give an empty name, which the window shows as "no folder" in its language.
pub fn project_of(cwd: &str) -> String {
    if cwd.to_lowercase().contains("scratch-workspaces") {
        return String::new();
    }
    cwd.split(['\\', '/']).rfind(|p| !p.is_empty()).unwrap_or(cwd).to_owned()
}

pub fn merge_rows(index: &[IndexEntry], files: &[SessionFile], acks: &HashMap<String, AckFile>, now: i64) -> Vec<Row> {
    let mut by_desktop: HashMap<&str, &SessionFile> = HashMap::new();
    let mut desktop_order: Vec<&str> = Vec::new();
    let mut loose: Vec<&SessionFile> = Vec::new();
    for f in files {
        match f.desktop_id.as_deref() {
            Some(id) => match by_desktop.get(id) {
                Some(held) if held.heartbeat_at >= f.heartbeat_at => {}
                Some(_) => {
                    by_desktop.insert(id, f);
                }
                None => {
                    by_desktop.insert(id, f);
                    desktop_order.push(id);
                }
            },
            None => loose.push(f),
        }
    }

    let mut rows = Vec::with_capacity(index.len() + loose.len());
    for s in index {
        let f = by_desktop.remove(s.desktop_id.as_str());
        rows.push(row_of(Some(s), f, acks, now));
    }
    for id in desktop_order {
        if let Some(f) = by_desktop.remove(id) {
            rows.push(row_of(None, Some(f), acks, now));
        }
    }
    for f in loose {
        rows.push(row_of(None, Some(f), acks, now));
    }

    rows.retain(|r| r.state != SessionState::Idle || now - r.last_activity_at < IDLE_KEEP_MS);
    rows.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)).then(b.last_activity_at.cmp(&a.last_activity_at)));
    rows
}

fn sort_key(r: &Row) -> u8 {
    match (r.needs_you, r.state) {
        (true, SessionState::Waiting) => 0,
        (true, SessionState::Done) => 1,
        (_, SessionState::Working) => 2,
        _ => 3,
    }
}

fn row_of(s: Option<&IndexEntry>, f: Option<&SessionFile>, acks: &HashMap<String, AckFile>, now: i64) -> Row {
    let is_live = f.is_some_and(|f| now - f.heartbeat_at < STALE_MS);
    let key = f.map(|f| f.key.clone()).or_else(|| s.map(|s| s.desktop_id.clone())).unwrap_or_else(|| "unknown".into());
    let last_activity_at = s.map_or(0, |s| s.last_activity_at).max(f.map_or(0, |f| f.heartbeat_at));
    // A file gone quiet keeps its words but not its state: the index says whether it runs.
    // A quiet file the app does not know (a closed terminal session) waits for nobody.
    let keeps_word = f.is_some_and(|f| {
        (s.is_some() || f.desktop_id.is_some()) && matches!(f.state, SessionState::Waiting | SessionState::Done)
    });
    let state = match (is_live, f) {
        (true, Some(f)) => f.state,
        _ if s.is_some_and(|s| s.is_running) => SessionState::Working,
        (_, Some(f)) if keeps_word => f.state,
        _ => SessionState::Idle,
    };
    let since = match f {
        Some(f) if is_live || state == f.state => f.state_since,
        _ => s.map_or(0, |s| s.last_activity_at),
    };
    let ack = acks.get(&safe_key(&key)).or_else(|| s.and_then(|s| acks.get(&safe_key(&s.desktop_id))));
    let needs_you = matches!(state, SessionState::Waiting | SessionState::Done) && !ack.is_some_and(|a| a.ack_at >= since);
    let cwd_of = |a: Option<&str>, b: Option<&str>| a.filter(|c| !c.is_empty()).or(b).unwrap_or("").to_owned();
    let index_title = s.map(|s| s.title.as_str()).filter(|t| !t.is_empty());
    let file_title = f.and_then(|f| f.title.as_deref()).filter(|t| !t.is_empty());
    let project = project_of(&cwd_of(s.map(|s| s.cwd.as_str()), f.map(|f| f.cwd.as_str())));
    let title = index_title
        .or(file_title)
        .map(str::to_owned)
        .unwrap_or_else(|| project_of(&cwd_of(f.map(|f| f.cwd.as_str()), s.map(|s| s.cwd.as_str()))));
    Row {
        key,
        desktop_id: s.map(|s| s.desktop_id.clone()).or_else(|| f.and_then(|f| f.desktop_id.clone())),
        title,
        project,
        link: s.and_then(|s| s.link.clone()).or_else(|| f.and_then(|f| f.link.clone())),
        state,
        ask: if state == SessionState::Waiting { f.and_then(|f| f.ask.clone()) } else { None },
        since,
        doing: if is_live && state == SessionState::Working { f.and_then(|f| f.doing.clone()) } else { None },
        needs_you,
        last_activity_at,
    }
}

pub fn counts(rows: &[Row]) -> Counts {
    let mut c = Counts { total: rows.len(), ..Counts::default() };
    for r in rows {
        match r.state {
            SessionState::Waiting if r.needs_you => c.waiting += 1,
            SessionState::Done if r.needs_you => c.done += 1,
            SessionState::Working => c.working += 1,
            _ => {}
        }
    }
    c
}

/// The flyout's groups: longest waiting first; finished and working, freshest first.
pub fn groups(rows: &[Row]) -> Groups<'_> {
    let pick = |state: SessionState| -> Vec<&Row> {
        rows.iter().filter(|r| r.state == state && (state == SessionState::Working || r.needs_you)).collect()
    };
    let mut waiting = pick(SessionState::Waiting);
    waiting.sort_by_key(|r| r.since);
    let mut done = pick(SessionState::Done);
    done.sort_by_key(|r| std::cmp::Reverse(r.last_activity_at));
    let mut working = pick(SessionState::Working);
    working.sort_by_key(|r| std::cmp::Reverse(r.last_activity_at));
    Groups { waiting, done, working }
}

/// The link a click may open: only the app's own `claude://` links.
pub fn open_target(row: &Row) -> Option<&str> {
    row.link.as_deref().filter(|l| l.starts_with("claude://"))
}
