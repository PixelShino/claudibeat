//! Reading the status-band folder and the two files claudibeat writes there.
//!
//! The mods write atomically, but a reader still meets torn files: a file that
//! does not parse keeps the value it had at the last good read.

use crate::board::{Assignment, AssignmentsFile, BoardFile};
use crate::model::{AccountFile, AckFile, IndexEntry, IndexFile, SessionFile, Versioned};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A file name the contract allows for a key: `local_…`, `cli-…`.
pub fn safe_key(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '_' })
        .collect()
}

#[derive(Default, Clone, Debug)]
pub struct Snapshot {
    pub index: Vec<IndexEntry>,
    pub files: Vec<SessionFile>,
    /// Keyed by the file name without `.json`.
    pub acks: HashMap<String, AckFile>,
    pub account: Option<AccountFile>,
    /// The Plane board the mod keeps; none without Plane.
    pub board: Option<BoardFile>,
    pub assignments: Vec<Assignment>,
}

/// Size and modification time: a file whose stamp has not moved is not read again.
type Stamp = (u64, Option<std::time::SystemTime>);

fn stamp_of(meta: &fs::Metadata) -> Stamp {
    (meta.len(), meta.modified().ok())
}

pub struct Store {
    dir: PathBuf,
    /// Raw text of the last good read of each path, with the file's stamp then.
    last_good: HashMap<PathBuf, (Stamp, String)>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir, last_good: HashMap::new() }
    }

    /// `%USERPROFILE%\.claude\status-band`.
    pub fn default_dir() -> PathBuf {
        let home = crate::win::shell::profile_dir();
        home.join(".claude").join("status-band")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn read(&mut self) -> Snapshot {
        let index_path = self.dir.join("index.json");
        let account_path = self.dir.join("account.json");
        let index = self.load::<IndexFile>(&index_path, fs::metadata(&index_path).ok().map(|m| stamp_of(&m))).map(|f| f.sessions).unwrap_or_default();
        let account = self.load::<AccountFile>(&account_path, fs::metadata(&account_path).ok().map(|m| stamp_of(&m)));
        let files = self.load_dir::<SessionFile>("sessions").into_iter().map(|(_, f)| f).collect();
        let acks = self.load_dir::<AckFile>("acks").into_iter().collect();
        let board_path = self.dir.join("board.json");
        let board = self.load::<BoardFile>(&board_path, fs::metadata(&board_path).ok().map(|m| stamp_of(&m)));
        let assign_path = self.dir.join("assignments.json");
        let assignments = self.load::<AssignmentsFile>(&assign_path, fs::metadata(&assign_path).ok().map(|m| stamp_of(&m))).map(|f| f.items).unwrap_or_default();
        Snapshot { index, files, acks, account, board, assignments }
    }

    pub fn write_ack(&self, key: &str, now_ms: i64) -> io::Result<()> {
        let text = format!(r#"{{"v":1,"ackAt":{now_ms},"by":"claudibeat"}}"#);
        write_atomic(&self.dir.join("acks").join(format!("{}.json", safe_key(key))), &text)
    }

    /// Tells the status-band mod that a tray speaks for it, so it stays silent.
    /// Without the mod's folder there is nobody to tell, and nothing is created.
    /// `quiet_helpers` asks the mod to take the app's blue dot off helper chats.
    pub fn write_tray(&self, now_ms: i64, quiet_helpers: bool) -> io::Result<()> {
        if !self.dir.is_dir() {
            return Ok(());
        }
        write_atomic(&self.dir.join("tray.json"), &format!(r#"{{"v":1,"at":{now_ms},"quietHelpers":{quiet_helpers}}}"#))
    }

    fn load_dir<T: DeserializeOwned + Versioned>(&mut self, sub: &str) -> Vec<(String, T)> {
        let Ok(entries) = fs::read_dir(self.dir.join(sub)) else { return Vec::new() };
        // On Windows the directory listing already carries size and time,
        // so stamping costs no extra call per file.
        let mut paths: Vec<(PathBuf, Option<Stamp>)> = entries
            .filter_map(|e| e.ok().map(|e| (e.path(), e.metadata().ok().map(|m| stamp_of(&m)))))
            .filter(|(p, _)| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        let present: Vec<PathBuf> = paths.iter().map(|(p, _)| p.clone()).collect();
        // Forget files that are gone, so a deleted file does not live on.
        let prefix = self.dir.join(sub);
        self.last_good.retain(|p, _| !p.starts_with(&prefix) || present.contains(p));
        paths
            .into_iter()
            .filter_map(|(p, stamp)| {
                let name = p.file_stem()?.to_string_lossy().into_owned();
                self.load::<T>(&p, stamp).map(|v| (name, v))
            })
            .collect()
    }

    fn load<T: DeserializeOwned + Versioned>(&mut self, path: &Path, stamp: Option<Stamp>) -> Option<T> {
        if let (Some(stamp), Some((seen, text))) = (stamp, self.last_good.get(path))
            && stamp == *seen
        {
            return parse::<T>(text);
        }
        match fs::read_to_string(path) {
            Ok(text) => {
                if let Some(value) = parse::<T>(&text) {
                    self.last_good.insert(path.to_path_buf(), (stamp.unwrap_or_default(), text));
                    return Some(value);
                }
            }
            // Gone means gone; anything else (a sharing violation, an
            // antivirus lock) is a moment, and the last good value stands.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.last_good.remove(path);
                return None;
            }
            Err(_) => {}
        }
        self.last_good.get(path).and_then(|(_, text)| parse::<T>(text))
    }
}

fn parse<T: DeserializeOwned + Versioned>(text: &str) -> Option<T> {
    serde_json::from_str::<T>(text).ok().filter(|v| v.version() == 1)
}

/// Write `<path>.tmp`, then rename over the target in one step.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}
