//! When the person last looked at each chat, as Claude Desktop itself records it.
//!
//! Claude keeps one metadata file per Code session,
//! `claude-code-sessions/<account>/<org>/local_<id>.json`, and stamps
//! `lastFocusedAt` there each time the person opens that chat. A chat that
//! waited or finished and was opened afterwards has been seen, whatever
//! claudibeat showed meanwhile. Only files whose size or time moved are read
//! again, and only those two fields are kept.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    session_id: String,
    #[serde(default)]
    last_focused_at: Option<i64>,
}

type Stamp = (u64, Option<SystemTime>);

#[derive(Default)]
pub struct FocusReader {
    roots: Vec<PathBuf>,
    cache: HashMap<PathBuf, (Stamp, Option<(String, i64)>)>,
}

impl FocusReader {
    /// Claude's session folders on this machine: the packaged app keeps them
    /// under `%LOCALAPPDATA%\Claude-Data\<package>\`, other installs under
    /// `%APPDATA%\Claude\`.
    pub fn new() -> FocusReader {
        let mut roots = vec![crate::win::shell::roaming_dir().join("Claude").join("claude-code-sessions")];
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| crate::win::shell::profile_dir().join("AppData").join("Local"));
        if let Ok(entries) = std::fs::read_dir(local.join("Claude-Data")) {
            roots.extend(entries.flatten().map(|e| e.path().join("claude-code-sessions")));
        }
        FocusReader::with_roots(roots)
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> FocusReader {
        FocusReader { roots, cache: HashMap::new() }
    }

    /// Desktop session id → when the person last opened it (Unix ms).
    pub fn read(&mut self) -> HashMap<String, i64> {
        let mut seen_paths = Vec::new();
        for root in &self.roots {
            for file in session_files(root) {
                seen_paths.push(file);
            }
        }
        let mut out = HashMap::new();
        for (path, stamp) in seen_paths {
            let fresh = self.cache.get(&path).is_none_or(|(s, _)| *s != stamp);
            if fresh {
                let parsed = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Meta>(&b).ok())
                    .and_then(|m| m.last_focused_at.filter(|t| *t > 0).map(|t| (m.session_id, t)));
                self.cache.insert(path.clone(), (stamp, parsed));
            }
            if let Some((_, Some((id, t)))) = self.cache.get(&path) {
                out.insert(id.clone(), *t);
            }
        }
        out
    }
}

/// `local_*.json` two folders under `root` (account, then organisation).
fn session_files(root: &Path) -> Vec<(PathBuf, Stamp)> {
    let mut out = Vec::new();
    let dirs = |p: &Path| -> Vec<PathBuf> { std::fs::read_dir(p).map(|r| r.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect()).unwrap_or_default() };
    for account in dirs(root) {
        for org in dirs(&account) {
            let Ok(entries) = std::fs::read_dir(&org) else { continue };
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with("local_") && name.ends_with(".json")) {
                    continue;
                }
                if let Ok(m) = e.metadata() {
                    out.push((e.path(), (m.len(), m.modified().ok())));
                }
            }
        }
    }
    out
}

/// The chat the person has open now: the one opened last, and only while
/// Claude is the window in front.
pub fn current(focus: &HashMap<String, i64>, claude_in_front: bool) -> Option<&str> {
    if !claude_in_front {
        return None;
    }
    focus.iter().max_by_key(|(_, t)| **t).map(|(id, _)| id.as_str())
}
