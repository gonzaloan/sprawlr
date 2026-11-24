//! What a person chose for the desktop app between runs. Every default is the one that costs
//! nothing and interrupts no one (ADR 0005).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// How often health is refreshed in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Every {
    #[serde(rename = "15m")]
    FifteenMinutes,
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "4h")]
    FourHours,
}

impl Every {
    pub fn duration(self) -> Duration {
        Duration::from_secs(match self {
            Every::FifteenMinutes => 15 * 60,
            Every::Hour => 3600,
            Every::FourHours => 4 * 3600,
        })
    }

    /// Refreshes a day at this interval, for the cost hint.
    pub fn per_day(self) -> u64 {
        86_400 / self.duration().as_secs()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    /// Off unless chosen: each refresh is billed by CloudWatch.
    pub background: Option<Every>,
    pub notifications: bool,
    /// `None` means the platform default (on for macOS and Windows, off for Linux).
    pub close_to_tray: Option<bool>,
    pub close_hint_shown: bool,
    /// The profile of the last scan or refresh. Kept here, not in the snapshot: snapshots
    /// get shared (ADR 0003).
    pub last_profile: Option<String>,
    /// Item keys already notified, and when, so the same item does not notify twice a day.
    pub notified: BTreeMap<String, DateTime<Utc>>,
}

/// How long an item stays "already notified".
pub const NOTIFY_AGAIN_AFTER_HOURS: i64 = 24;

impl DesktopSettings {
    /// Keys not notified in the last day, after forgetting older entries.
    pub fn fresh_keys(&mut self, keys: &[String], now: DateTime<Utc>) -> Vec<String> {
        let window = chrono::Duration::hours(NOTIFY_AGAIN_AFTER_HOURS);
        self.notified.retain(|_, at| now - *at < window);
        keys.iter().filter(|k| !self.notified.contains_key(*k)).cloned().collect()
    }

    pub fn remember_notified(&mut self, keys: &[String], now: DateTime<Utc>) {
        for k in keys {
            self.notified.insert(k.clone(), now);
        }
    }
}

/// Wait before the next background refresh: the interval, doubled after each failure in a
/// row, never more than a day.
pub fn next_wait(every: Every, failures: u32) -> Duration {
    let base = every.duration().as_secs();
    let wait = base.saturating_mul(1u64 << failures.min(16));
    Duration::from_secs(wait.min(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cost_nothing_and_stay_quiet() {
        let s = DesktopSettings::default();
        assert_eq!(s.background, None);
        assert!(!s.notifications);
        let parsed: DesktopSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, s, "an empty file is the defaults");
        assert_eq!(serde_json::to_value(Every::FifteenMinutes).unwrap(), "15m");
    }

    #[test]
    fn back_off_doubles_up_to_a_day() {
        let h = |n: u32| next_wait(Every::Hour, n).as_secs() / 3600;
        assert_eq!([h(0), h(1), h(2), h(3)], [1, 2, 4, 8]);
        assert_eq!(next_wait(Every::FourHours, 3).as_secs(), 86_400);
        assert_eq!(next_wait(Every::FifteenMinutes, 40).as_secs(), 86_400);
        assert_eq!(Every::FifteenMinutes.per_day(), 96);
    }

    #[test]
    fn an_item_notifies_once_a_day() {
        let mut s = DesktopSettings::default();
        let t = Utc::now();
        let keys = vec!["a".to_string()];
        assert_eq!(s.fresh_keys(&keys, t), keys);
        s.remember_notified(&keys, t);
        assert!(s.fresh_keys(&keys, t + chrono::Duration::hours(3)).is_empty());
        assert_eq!(s.fresh_keys(&keys, t + chrono::Duration::hours(25)), keys);
        assert!(s.notified.is_empty(), "old entries are forgotten");
    }
}
