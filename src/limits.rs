//! The plan's usage windows, shown as the part already used (the owner's
//! choice: a ring that fills up reads faster than one that drains).

use crate::model::AccountFile;

/// `account.json` older than this is not shown: the figures may be long gone.
pub const STALE_ACCOUNT_MS: i64 = 6 * 3_600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitWindow {
    FiveHour,
    Week,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitView {
    pub window: LimitWindow,
    pub used: u8,
    pub remaining: u8,
    pub resets_at: Option<i64>,
}

impl LimitView {
    /// Fraction of the window already used: how full the ring is.
    pub fn used_fraction(&self) -> f32 {
        f32::from(self.used) / 100.0
    }
}

pub fn limit_views(account: Option<&AccountFile>, now: i64) -> Vec<LimitView> {
    let Some(account) = account.filter(|a| now - a.updated_at <= STALE_ACCOUNT_MS) else {
        return Vec::new();
    };
    account
        .limits
        .iter()
        .filter_map(|l| {
            let window = match l.window.as_str() {
                "five_hour" => LimitWindow::FiveHour,
                "seven_day" => LimitWindow::Week,
                _ => return None,
            };
            let used = l.percent.round().clamp(0.0, 100.0) as u8;
            Some(LimitView { window, used, remaining: 100 - used, resets_at: l.resets_at })
        })
        .collect()
}

/// The strip shows the fullest window: the one closest to running out.
pub fn strip_limit(views: &[LimitView]) -> Option<&LimitView> {
    views.iter().max_by_key(|v| v.used)
}
