//! Aim session history and trends.
//!
//! Every finished aim test is stored with the sensitivity it was played at, so the
//! app can answer "at which cm/360 do I actually aim best?" from real data.
//! Stored in `%APPDATA%\SensMatch\sessions.json`.

use crate::aim::{AimMode, AimReport};
use crate::error::AppResult;
use crate::profiles::now_unix;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_SESSIONS: usize = 500;
const DAY: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionRecord {
    pub id: u64,
    pub finished: u64,
    pub mode: AimMode,
    pub game: String,
    pub sens: f64,
    pub cm360: f64,
    pub accuracy: f64,
    pub avg_reaction_ms: f64,
    pub avg_ttk_ms: f64,
    pub overshoot_pct: f64,
    pub score: u32,
}

impl SessionRecord {
    pub fn from_report(report: &AimReport, game: &str, sens: f64, cm360: f64) -> Self {
        let l = &report.live;
        Self {
            id: 0,
            finished: now_unix(),
            mode: report.config.mode,
            game: game.to_string(),
            sens,
            cm360,
            accuracy: l.accuracy,
            avg_reaction_ms: l.avg_reaction_ms,
            avg_ttk_ms: l.avg_ttk_ms,
            overshoot_pct: l.overshoot_pct,
            score: l.score,
        }
    }

    /// Mode-independent quality: accuracy per second of TTK, minus overshoot.
    pub fn quality(&self) -> f64 {
        self.accuracy * 1000.0 / self.avg_ttk_ms.max(1.0) * (1.0 - (self.overshoot_pct / 100.0).clamp(0.0, 0.9))
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Trend {
    pub sessions_7d: u32,
    pub avg_accuracy_7d: f64,
    /// Latest accuracy minus the 7-day average, in percentage points.
    pub accuracy_delta_pts: f64,
    /// Latest TTK minus the previous session's TTK (negative = faster).
    pub ttk_delta_ms: f64,
    pub best_score: u32,
    /// cm/360 bucket with the best average quality (needs 3+ sessions).
    pub best_cm360: Option<f64>,
}

#[derive(Debug, Default)]
pub struct HistoryStore {
    path: Option<PathBuf>,
    pub items: Vec<SessionRecord>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("SensMatch").join("sessions.json"))
}

impl HistoryStore {
    pub fn open(path: Option<PathBuf>) -> Self {
        let items = path
            .as_deref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, items }
    }

    pub fn add(&mut self, mut rec: SessionRecord) -> AppResult<SessionRecord> {
        rec.id = self.items.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        self.items.insert(0, rec.clone());
        self.items.truncate(MAX_SESSIONS);
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, serde_json::to_vec_pretty(&self.items)?)?;
        }
        Ok(rec)
    }

    /// Newest first, limited.
    pub fn recent(&self, limit: usize) -> Vec<SessionRecord> {
        self.items.iter().take(limit).cloned().collect()
    }

    pub fn trend(&self, now: u64) -> Trend {
        let Some(latest) = self.items.first() else { return Trend::default() };
        let week: Vec<&SessionRecord> = self.items.iter().filter(|r| now.saturating_sub(r.finished) <= 7 * DAY).collect();
        let avg_acc = if week.is_empty() { latest.accuracy } else { week.iter().map(|r| r.accuracy).sum::<f64>() / week.len() as f64 };
        let ttk_delta = self.items.get(1).map(|prev| latest.avg_ttk_ms - prev.avg_ttk_ms).unwrap_or(0.0);
        Trend {
            sessions_7d: week.len() as u32,
            avg_accuracy_7d: avg_acc,
            accuracy_delta_pts: (latest.accuracy - avg_acc) * 100.0,
            ttk_delta_ms: ttk_delta,
            best_score: self.items.iter().map(|r| r.score).max().unwrap_or(0),
            best_cm360: best_cm360(&self.items, 2.0),
        }
    }
}

/// Group sessions into `bucket_cm` wide cm/360 buckets and return the centre of the
/// bucket with the highest mean quality. Buckets need at least 3 sessions.
pub fn best_cm360(items: &[SessionRecord], bucket_cm: f64) -> Option<f64> {
    let mut buckets: Vec<(i64, f64, u32)> = Vec::new();
    for r in items.iter().filter(|r| r.cm360 > 0.0) {
        let key = (r.cm360 / bucket_cm).floor() as i64;
        match buckets.iter_mut().find(|b| b.0 == key) {
            Some(b) => {
                b.1 += r.quality();
                b.2 += 1;
            }
            None => buckets.push((key, r.quality(), 1)),
        }
    }
    buckets
        .into_iter()
        .filter(|b| b.2 >= 3)
        .max_by(|a, b| (a.1 / a.2 as f64).total_cmp(&(b.1 / b.2 as f64)))
        .map(|b| (b.0 as f64 + 0.5) * bucket_cm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(age_days: u64, cm: f64, acc: f64, ttk: f64, score: u32) -> SessionRecord {
        SessionRecord {
            id: 0, finished: 10 * DAY - age_days * DAY, mode: AimMode::Flick, game: "cs2".into(), sens: 1.0,
            cm360: cm, accuracy: acc, avg_reaction_ms: 270.0, avg_ttk_ms: ttk, overshoot_pct: 5.0, score,
        }
    }

    #[test]
    fn trend_compares_latest_with_week_and_previous() {
        let mut h = HistoryStore::default();
        for r in [rec(9, 40.0, 0.70, 500.0, 9000), rec(2, 40.0, 0.86, 470.0, 12000), rec(0, 40.0, 0.92, 452.0, 14000)] {
            h.add(r).unwrap();
        }
        let t = h.trend(10 * DAY);
        assert_eq!(t.sessions_7d, 2);
        assert!((t.avg_accuracy_7d - 0.89).abs() < 1e-9);
        assert!((t.accuracy_delta_pts - 3.0).abs() < 1e-9);
        assert!((t.ttk_delta_ms + 18.0).abs() < 1e-9);
        assert_eq!(t.best_score, 14000);
        assert_eq!(h.items[0].id, 3);
    }

    #[test]
    fn best_bucket_needs_enough_sessions() {
        let mut items = vec![rec(0, 50.5, 1.0, 300.0, 0)];
        assert_eq!(best_cm360(&items, 2.0), None);
        for _ in 0..3 {
            items.push(rec(0, 40.4, 0.92, 430.0, 0));
            items.push(rec(0, 33.1, 0.85, 470.0, 0));
        }
        assert_eq!(best_cm360(&items, 2.0), Some(41.0));
    }

    #[test]
    fn empty_history_has_default_trend() {
        let t = HistoryStore::default().trend(now_unix());
        assert_eq!(t.sessions_7d, 0);
        assert!(t.best_cm360.is_none());
    }
}
