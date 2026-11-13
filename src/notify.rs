//! When to speak up, and how the toasts above the strip share three places.
//!
//! Rules from the design: no toasts for what was already pending at start-up,
//! one toast per session per 10 s, none for a session just opened from here,
//! finished sessions gathered over 2 s into one toast, and someone waiting
//! never pushed out by someone finished.

use crate::merge::Row;
use crate::model::SessionState;
use std::collections::{HashMap, HashSet};

pub const THROTTLE_MS: i64 = 10_000;
pub const OPENED_QUIET_MS: i64 = 5_000;
pub const DONE_GATHER_MS: i64 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Waiting,
    Done,
    /// Several sessions finished within the gathering window.
    DoneBatch,
    /// The last place, once three people wait: «и ещё N ждут ответа».
    MoreWaiting,
}

impl ToastKind {
    pub fn is_waiting(self) -> bool {
        matches!(self, ToastKind::Waiting | ToastKind::MoreWaiting)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastEvent {
    pub kind: ToastKind,
    pub keys: Vec<String>,
    pub at: i64,
    /// Sessions behind the toast: 1, or the size of a batch or summary.
    pub count: usize,
}

#[derive(Default)]
pub struct Notifier {
    prev: Option<HashMap<String, SessionState>>,
    last_fired: HashMap<String, i64>,
    opened: HashMap<String, i64>,
    pending_done: Vec<String>,
    pending_since: i64,
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier::default()
    }

    /// The person opened this session from claudibeat.
    pub fn opened(&mut self, key: &str, now: i64) {
        self.opened.insert(key.to_owned(), now);
        self.pending_done.retain(|k| k != key);
    }

    pub fn observe(&mut self, rows: &[Row], now: i64) -> Vec<ToastEvent> {
        let current: HashMap<String, SessionState> =
            rows.iter().filter(|r| r.needs_you).map(|r| (r.key.clone(), r.state)).collect();
        let Some(prev) = self.prev.replace(current.clone()) else { return Vec::new() };

        let mut out = Vec::new();
        let mut fresh: Vec<(&String, &SessionState)> =
            current.iter().filter(|(k, s)| prev.get(*k) != Some(*s)).collect();
        fresh.sort_by_key(|(k, _)| *k);
        for (key, state) in fresh {
            if self.opened.get(key).is_some_and(|&t| now - t < OPENED_QUIET_MS)
                || self.last_fired.get(key).is_some_and(|&t| now - t < THROTTLE_MS)
            {
                continue;
            }
            match state {
                SessionState::Waiting => {
                    self.last_fired.insert(key.clone(), now);
                    self.pending_done.retain(|k| k != key);
                    out.push(ToastEvent { kind: ToastKind::Waiting, keys: vec![key.clone()], at: now, count: 1 });
                }
                SessionState::Done if !self.pending_done.contains(key) => {
                    if self.pending_done.is_empty() {
                        self.pending_since = now;
                    }
                    self.pending_done.push(key.clone());
                }
                _ => {}
            }
        }

        self.pending_done.retain(|k| current.get(k) == Some(&SessionState::Done));
        if !self.pending_done.is_empty() && now - self.pending_since >= DONE_GATHER_MS {
            let keys = std::mem::take(&mut self.pending_done);
            for k in &keys {
                self.last_fired.insert(k.clone(), now);
            }
            let kind = if keys.len() == 1 { ToastKind::Done } else { ToastKind::DoneBatch };
            out.push(ToastEvent { kind, count: keys.len(), keys, at: now });
        }
        out
    }
}

#[derive(Default, Debug)]
pub struct ToastStack {
    /// Oldest first; the newest sits lowest, closest to the strip.
    pub items: Vec<ToastEvent>,
}

impl ToastStack {
    pub const CAPACITY: usize = 3;

    pub fn push(&mut self, ev: ToastEvent) {
        if self.items.len() < Self::CAPACITY {
            self.items.push(ev);
            return;
        }
        if let Some(i) = self.items.iter().position(|e| !e.kind.is_waiting()) {
            self.items.remove(i);
            self.items.push(ev);
            return;
        }
        if !ev.kind.is_waiting() {
            return; // every place holds someone waiting
        }
        let last = self.items.last_mut().expect("the stack is full");
        if last.kind != ToastKind::MoreWaiting {
            last.kind = ToastKind::MoreWaiting;
        }
        last.keys.extend(ev.keys);
        last.count = last.keys.len();
        last.at = ev.at;
    }

    pub fn remove_at(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
        }
    }

    /// Drop toasts whose sessions no longer need the person (opened elsewhere).
    pub fn retain_needing(&mut self, needing: &HashSet<String>) {
        for e in &mut self.items {
            e.keys.retain(|k| needing.contains(k));
            if e.kind == ToastKind::MoreWaiting {
                e.count = e.keys.len();
            }
        }
        self.items.retain(|e| !e.keys.is_empty());
    }
}
