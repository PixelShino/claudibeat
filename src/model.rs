//! The files of the status-band folder (contract v1 of the status-band mod).
//! Field names follow the files; anything the files add later is ignored.

use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Working,
    Waiting,
    Done,
    Idle,
}

/// `sessions/<key>.json`: written by the session's own mod.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub v: u32,
    pub key: String,
    #[serde(default)]
    pub desktop_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub link: Option<String>,
    pub state: SessionState,
    #[serde(default)]
    pub ask: Option<String>,
    #[serde(default)]
    pub state_since: i64,
    #[serde(default)]
    pub doing: Option<String>,
    #[serde(default)]
    pub heartbeat_at: i64,
}

/// One session of the app, from `index.json`.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub desktop_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub is_running: bool,
    #[serde(default)]
    pub last_activity_at: i64,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexFile {
    pub v: u32,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub sessions: Vec<IndexEntry>,
}

/// `acks/<key>.json`: the person has seen the session's state since `ack_at`.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AckFile {
    pub v: u32,
    pub ack_at: i64,
    #[serde(default)]
    pub by: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    pub window: String,
    /// Percent of the window already used.
    pub percent: f64,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

/// `account.json`: the plan's usage windows.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountFile {
    pub v: u32,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub limits: Vec<Limit>,
}

/// Every file of the contract carries `v`; only version 1 is understood.
pub trait Versioned {
    fn version(&self) -> u32;
}

macro_rules! versioned {
    ($($t:ty),*) => { $(impl Versioned for $t { fn version(&self) -> u32 { self.v } })* };
}
versioned!(SessionFile, IndexFile, AckFile, AccountFile);
