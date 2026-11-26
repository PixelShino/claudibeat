//! The Plane board the status-band mod keeps in `board.json`, and the
//! requests claudibeat leaves for the mod (`requests/`, `replies/`).
//!
//! claudibeat still never touches the network: the mod fetches Plane, moves
//! work items and hands them to chats; claudibeat only reads what it wrote and
//! drops requests beside it. The contract is in
//! `docs/superpowers/specs/2026-11-17-plane-board-design.md`.

use serde::Deserialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

impl crate::model::Versioned for BoardFile {
    fn version(&self) -> u32 {
        self.v
    }
}

impl crate::model::Versioned for AssignmentsFile {
    fn version(&self) -> u32 {
        self.v
    }
}

/// A request nobody answered for this long is withdrawn.
pub const REQUEST_TIMEOUT_MS: i64 = 120_000;
/// A reply stays this long after its request was made.
pub const REPLY_KEEP_MS: i64 = REQUEST_TIMEOUT_MS + 60_000;

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub state_name: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub url: String,
}

/// `board.json`, written by the mod that leads.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoardFile {
    pub v: u32,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub todo: Vec<Card>,
    #[serde(default)]
    pub doing: Vec<Card>,
    #[serde(default)]
    pub done_today: Vec<Card>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub task: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub session_title: String,
    #[serde(default)]
    pub at: i64,
}

/// `assignments.json`: which chat holds which work item.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentsFile {
    pub v: u32,
    #[serde(default)]
    pub items: Vec<Assignment>,
}

/// `replies/<id>.json`: the mod's word on one request.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Reply {
    pub v: u32,
    pub id: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub text: String,
    /// An assign's result: the work item and the chat that took it.
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default, rename = "sessionTitle")]
    pub session_title: Option<String>,
}

impl Reply {
    /// Who holds the card now, if this reply says so.
    pub fn assignment(&self) -> Option<Assignment> {
        let task = self.task.clone().filter(|t| !t.is_empty())?;
        let session_title = self.session_title.clone().unwrap_or_default();
        Some(Assignment { task, session: self.session.clone().unwrap_or_default(), session_title, at: self.at })
    }
}

/// The board's three task columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Todo,
    Doing,
    Done,
}

impl Column {
    pub const ALL: [Column; 3] = [Column::Todo, Column::Doing, Column::Done];

    /// The Plane move that puts a card in this column.
    pub fn move_to(self) -> &'static str {
        match self {
            Column::Todo => "todo",
            Column::Doing => "doing",
            Column::Done => "done",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Card {
    /// A state of the «started» group that means the work is done and waits
    /// for a look («На проверке», «Review»): the board shows it with the done.
    pub fn in_review(&self) -> bool {
        let s = self.state_name.to_lowercase();
        s.contains("провер") || s.contains("review") || s.contains("ревью")
    }
}

impl BoardFile {
    /// The cards a column shows: in-review work sits with the done, first.
    pub fn column(&self, c: Column) -> Vec<&Card> {
        match c {
            Column::Todo => self.todo.iter().collect(),
            Column::Doing => self.doing.iter().filter(|k| !k.in_review()).collect(),
            Column::Done => self.doing.iter().filter(|k| k.in_review()).chain(&self.done_today).collect(),
        }
    }

    /// The column a card is shown in.
    pub fn column_of(&self, key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|&c| self.column(c).iter().any(|k| k.key == key))
    }

    /// The column of a card's Plane state: an in-review card is still in
    /// progress there, so dropping it on «done» does close it.
    pub fn state_column(&self, key: &str) -> Option<Column> {
        if self.doing.iter().any(|k| k.key == key) {
            return Some(Column::Doing);
        }
        self.column_of(key)
    }

    pub fn card(&self, key: &str) -> Option<&Card> {
        self.todo.iter().chain(&self.doing).chain(&self.done_today).find(|k| k.key == key)
    }

    /// The cards of one column the filter lets through.
    pub fn filtered(&self, c: Column, project: Option<&str>) -> Vec<&Card> {
        self.column(c).into_iter().filter(|k| project.is_none_or(|p| k.project == p)).collect()
    }
}

/// Where a dragged card goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An open chat, by its `local_…` key.
    Session(String),
    /// A new chat in this folder (`cwd` empty: no folder).
    NewChat { cwd: String, project: String },
    /// Another column: the work item changes state.
    Column(Column),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Assign { task: String, target: Target },
    Move { task: String, to: Column },
    Refresh,
}

/// A request on its way, until its reply comes or it times out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    pub at: i64,
    /// The work item it concerns; none for a refresh.
    pub task: Option<String>,
    /// What the card says meanwhile: «отправляю…».
    pub label: String,
}

/// `cb-<ms>-<4 hex>`: unique enough for one person's clicks.
pub fn request_id(now_ms: i64, salt: u32) -> String {
    format!("cb-{now_ms}-{:04x}", salt & 0xFFFF)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The request as the mod reads it.
pub fn request_json(id: &str, now_ms: i64, r: &Request) -> String {
    let head = format!(r#""v":1,"id":{},"at":{now_ms},"by":"claudibeat""#, json_str(id));
    match r {
        Request::Assign { task, target } => {
            let target = match target {
                Target::Session(key) => format!(r#"{{"session":{}}}"#, json_str(key)),
                Target::NewChat { cwd, project } => format!(r#"{{"newChat":{{"cwd":{},"project":{}}}}}"#, json_str(cwd), json_str(project)),
                // A column is not a chat: the caller sends a Move instead.
                Target::Column(c) => return request_json(id, now_ms, &Request::Move { task: task.clone(), to: *c }),
            };
            format!(r#"{{{head},"kind":"assign","task":{},"target":{target}}}"#, json_str(task))
        }
        Request::Move { task, to } => format!(r#"{{{head},"kind":"move","task":{},"to":"{}"}}"#, json_str(task), to.move_to()),
        Request::Refresh => format!(r#"{{{head},"kind":"refresh"}}"#),
    }
}

/// Requests out and replies in, beside the mod's files.
pub struct Mailbox {
    dir: PathBuf,
}

impl Mailbox {
    pub fn new(dir: &Path) -> Mailbox {
        Mailbox { dir: dir.to_path_buf() }
    }

    fn request_path(&self, id: &str) -> PathBuf {
        self.dir.join("requests").join(format!("{}.json", crate::store::safe_key(id)))
    }

    fn reply_path(&self, id: &str) -> PathBuf {
        self.dir.join("replies").join(format!("{}.json", crate::store::safe_key(id)))
    }

    pub fn send(&self, id: &str, now_ms: i64, r: &Request) -> io::Result<()> {
        crate::store::write_atomic(&self.request_path(id), &request_json(id, now_ms, r))
    }

    /// The reply to `id`, once the mod has written a whole one.
    pub fn reply(&self, id: &str) -> Option<Reply> {
        let text = fs::read_to_string(self.reply_path(id)).ok()?;
        serde_json::from_str::<Reply>(&text).ok().filter(|r| r.v == 1 && r.id == id)
    }

    /// Done with a request: it goes at once. Its reply stays a while, since
    /// the leading mod reads other sessions' replies to keep `assignments.json`;
    /// [`Mailbox::sweep`] takes it later.
    pub fn forget(&self, id: &str) {
        let _ = fs::remove_file(self.request_path(id));
    }

    /// Replies and requests of ours left from an earlier run (a crash, a
    /// quit before the answer) go once they are older than the timeout.
    pub fn sweep(&self, now_ms: i64) {
        for sub in ["requests", "replies", "claims"] {
            let Ok(entries) = fs::read_dir(self.dir.join(sub)) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                // Ours are `cb-<ms>-<salt>`; an agent closing a task writes `ag-<ms>`.
                let stem = name.trim_end_matches(".json");
                let Some(ms) = stem.strip_prefix("cb-").or_else(|| stem.strip_prefix("ag-")).and_then(|s| s.split('-').next()).and_then(|s| s.parse::<i64>().ok()) else { continue };
                let keep = if sub == "replies" { REPLY_KEEP_MS } else { REQUEST_TIMEOUT_MS * 5 };
                if now_ms - ms > keep {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }
}

/// The chat a work item is with, if any.
pub fn holder<'a>(assignments: &'a [Assignment], task: &str) -> Option<&'a Assignment> {
    assignments.iter().rev().find(|a| a.task == task)
}

/// Named project folders, freshest first, for «new agent here».
pub fn project_folders(rows: &[(String, String, i64)], limit: usize) -> Vec<(String, String)> {
    let mut sorted: Vec<&(String, String, i64)> = rows.iter().filter(|(cwd, _, _)| !cwd.is_empty()).collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.2));
    let mut out: Vec<(String, String)> = Vec::new();
    for (cwd, project, _) in sorted {
        if out.len() >= limit {
            break;
        }
        if !out.iter().any(|(c, _)| c.eq_ignore_ascii_case(cwd)) {
            out.push((cwd.clone(), project.clone()));
        }
    }
    out
}
