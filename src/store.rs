//! Reading the status-band folder and the two files claudibeat writes there.
//!
//! The mods write atomically, but a reader still meets torn files: a file that
//! does not parse keeps the value it had at the last good read.

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
}

pub struct Store {
    dir: PathBuf,
    /// Raw text of the last good read of each path.
    last_good: HashMap<PathBuf, String>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir, last_good: HashMap::new() }
    }

    /// `%USERPROFILE%\.claude\status-band`.
    pub fn default_dir() -> PathBuf {
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
        home.join(".claude").join("status-band")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn read(&mut self) -> Snapshot {
        let index = self.load::<IndexFile>(&self.dir.join("index.json")).map(|f| f.sessions).unwrap_or_default();
        let account = self.load::<AccountFile>(&self.dir.join("account.json"));
        let files = self.load_dir::<SessionFile>("sessions").into_iter().map(|(_, f)| f).collect();
        let acks = self.load_dir::<AckFile>("acks").into_iter().collect();
        Snapshot { index, files, acks, account }
    }

    pub fn write_ack(&self, key: &str, now_ms: i64) -> io::Result<()> {
        let text = format!(r#"{{"v":1,"ackAt":{now_ms},"by":"claudibeat"}}"#);
        write_atomic(&self.dir.join("acks").join(format!("{}.json", safe_key(key))), &text)
    }

    /// Tells the status-band mod that a tray speaks for it, so it stays silent.
    /// Without the mod's folder there is nobody to tell, and nothing is created.
    pub fn write_tray(&self, now_ms: i64) -> io::Result<()> {
        if !self.dir.is_dir() {
            return Ok(());
        }
        write_atomic(&self.dir.join("tray.json"), &format!(r#"{{"v":1,"at":{now_ms}}}"#))
    }

    fn load_dir<T: DeserializeOwned + Versioned>(&mut self, sub: &str) -> Vec<(String, T)> {
        let Ok(entries) = fs::read_dir(self.dir.join(sub)) else { return Vec::new() };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        let present: Vec<PathBuf> = paths.clone();
        // Forget files that are gone, so a deleted file does not live on.
        let prefix = self.dir.join(sub);
        self.last_good.retain(|p, _| !p.starts_with(&prefix) || present.contains(p));
        paths
            .into_iter()
            .filter_map(|p| {
                let name = p.file_stem()?.to_string_lossy().into_owned();
                self.load::<T>(&p).map(|v| (name, v))
            })
            .collect()
    }

    fn load<T: DeserializeOwned + Versioned>(&mut self, path: &Path) -> Option<T> {
        match fs::read_to_string(path) {
            Ok(text) => {
                if let Some(value) = parse::<T>(&text) {
                    self.last_good.insert(path.to_path_buf(), text);
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
        self.last_good.get(path).and_then(|text| parse::<T>(text))
    }
}

fn parse<T: DeserializeOwned + Versioned>(text: &str) -> Option<T> {
    serde_json::from_str::<T>(text).ok().filter(|v| v.version() == 1)
}

/// Write `<path>.tmp`, then rename over the target in one step.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}
