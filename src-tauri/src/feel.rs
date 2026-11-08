//! Feel Match: find the cm/360 that actually feels right with A/B flick tests.
//!
//! The search runs in log space (sensitivity perception is roughly logarithmic).
//! Each round offers two candidates at the lower and upper quartile of the current
//! range. Picking one keeps the half of the range that contains it; "same" keeps
//! the middle half. Either way the range halves, so 8 rounds shrink a +/-35%
//! window down to about +/-0.2%. The search stops at the tolerance or round cap.

use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Candidate A (lower cm/360 = faster) felt better.
    A,
    /// Candidate B (higher cm/360 = slower) felt better.
    B,
    Same,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FeelConfig {
    pub base_cm360: f64,
    /// Half-width of the initial window as a fraction (0.35 = +/-35%).
    pub spread: f64,
    /// Stop when the range is narrower than this (percent of its centre).
    pub tolerance_pct: f64,
    pub max_rounds: u32,
}

impl Default for FeelConfig {
    fn default() -> Self {
        Self { base_cm360: 40.0, spread: 0.35, tolerance_pct: 4.0, max_rounds: 8 }
    }
}

/// Objective numbers from one short flick run, used for the optional auto-pick.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlickScore {
    pub accuracy: f64,
    pub avg_ttk_ms: f64,
    pub overshoot_pct: f64,
}

impl FlickScore {
    /// Higher is better: accuracy per second of TTK, penalised by overshoot.
    pub fn composite(&self) -> f64 {
        let ttk = self.avg_ttk_ms.max(1.0);
        self.accuracy.clamp(0.0, 1.0) * 1000.0 / ttk * (1.0 - (self.overshoot_pct / 100.0).clamp(0.0, 0.9))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Round {
    pub index: u32,
    pub lo: f64,
    pub hi: f64,
    pub a_cm360: f64,
    pub b_cm360: f64,
    pub choice: Option<Choice>,
    pub score_a: Option<FlickScore>,
    pub score_b: Option<FlickScore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeelEstimate {
    pub cm360: f64,
    pub lo: f64,
    pub hi: f64,
    /// +/- half-width of the remaining range in percent.
    pub uncertainty_pct: f64,
    pub rounds_used: u32,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeelSession {
    pub config: FeelConfig,
    pub lo: f64,
    pub hi: f64,
    pub rounds: Vec<Round>,
    pub done: bool,
}

/// Point at fraction `t` between `lo` and `hi` in log space.
fn log_lerp(lo: f64, hi: f64, t: f64) -> f64 {
    (lo.ln() + (hi.ln() - lo.ln()) * t).exp()
}

impl FeelSession {
    pub fn new(config: FeelConfig) -> Result<Self, AppError> {
        if !(config.base_cm360 > 1.0 && config.base_cm360 < 500.0) {
            return Err(AppError::Invalid("base cm/360 must be between 1 and 500".into()));
        }
        if !(config.spread > 0.01 && config.spread < 0.9) {
            return Err(AppError::Invalid("spread must be between 1% and 90%".into()));
        }
        let lo = config.base_cm360 * (1.0 - config.spread);
        let hi = config.base_cm360 * (1.0 + config.spread);
        let mut s = Self { config, lo, hi, rounds: Vec::new(), done: false };
        s.open_round();
        Ok(s)
    }

    pub fn width_pct(&self) -> f64 {
        (self.hi / self.lo - 1.0) * 100.0
    }

    fn open_round(&mut self) {
        let index = self.rounds.len() as u32 + 1;
        self.rounds.push(Round {
            index,
            lo: self.lo,
            hi: self.hi,
            a_cm360: log_lerp(self.lo, self.hi, 0.25),
            b_cm360: log_lerp(self.lo, self.hi, 0.75),
            choice: None,
            score_a: None,
            score_b: None,
        });
    }

    pub fn current(&self) -> Option<&Round> {
        self.rounds.last().filter(|r| r.choice.is_none())
    }

    /// Attach measured flick scores for the open round.
    pub fn record_scores(&mut self, a: Option<FlickScore>, b: Option<FlickScore>) {
        if let Some(r) = self.rounds.last_mut().filter(|r| r.choice.is_none()) {
            if a.is_some() {
                r.score_a = a;
            }
            if b.is_some() {
                r.score_b = b;
            }
        }
    }

    /// Suggest a choice from measured scores (`None` until both are present).
    pub fn suggestion(&self) -> Option<Choice> {
        let r = self.current()?;
        let (a, b) = (r.score_a?.composite(), r.score_b?.composite());
        let diff = (a - b) / a.max(b).max(1e-9);
        Some(if diff.abs() < 0.03 { Choice::Same } else if diff > 0.0 { Choice::A } else { Choice::B })
    }

    pub fn submit(&mut self, choice: Choice) -> Result<FeelEstimate, AppError> {
        if self.done {
            return Err(AppError::Invalid("Feel Match is already finished".into()));
        }
        let (lo, hi) = (self.lo, self.hi);
        let mid = log_lerp(lo, hi, 0.5);
        let round = self.rounds.last_mut().ok_or_else(|| AppError::Invalid("no open round".into()))?;
        round.choice = Some(choice);
        match choice {
            Choice::A => self.hi = mid,
            Choice::B => self.lo = mid,
            Choice::Same => {
                self.lo = round.a_cm360;
                self.hi = round.b_cm360;
            }
        }
        let used = self.rounds.len() as u32;
        if self.width_pct() <= self.config.tolerance_pct || used >= self.config.max_rounds {
            self.done = true;
        } else {
            self.open_round();
        }
        Ok(self.estimate())
    }

    /// Step back one decision.
    pub fn undo(&mut self) -> bool {
        if self.rounds.len() > 1 && !self.done {
            self.rounds.pop();
        }
        match self.rounds.last_mut() {
            Some(r) if r.choice.is_some() => {
                r.choice = None;
                self.lo = r.lo;
                self.hi = r.hi;
                self.done = false;
                true
            }
            _ => false,
        }
    }

    pub fn estimate(&self) -> FeelEstimate {
        let cm = log_lerp(self.lo, self.hi, 0.5);
        FeelEstimate {
            cm360: cm,
            lo: self.lo,
            hi: self.hi,
            uncertainty_pct: (self.hi - self.lo) / 2.0 / cm * 100.0,
            rounds_used: self.rounds.iter().filter(|r| r.choice.is_some()).count() as u32,
            done: self.done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perfect tester: prefers whichever candidate is closer (in log space).
    fn oracle(r: &Round, truth: f64) -> Choice {
        let da = (r.a_cm360.ln() - truth.ln()).abs();
        let db = (r.b_cm360.ln() - truth.ln()).abs();
        if (da - db).abs() < 1e-9 { Choice::Same } else if da < db { Choice::A } else { Choice::B }
    }

    #[test]
    fn converges_on_true_preference() {
        for truth in [27.0, 31.5, 38.0, 40.0, 44.2, 52.0] {
            let mut s = FeelSession::new(FeelConfig { tolerance_pct: 1.0, max_rounds: 12, ..Default::default() }).unwrap();
            while !s.done {
                let c = oracle(s.current().unwrap(), truth);
                s.submit(c).unwrap();
                assert!(s.lo <= truth + 1e-9 && truth <= s.hi + 1e-9, "lost {truth}: {}..{}", s.lo, s.hi);
            }
            let e = s.estimate();
            assert!((e.cm360 / truth - 1.0).abs() < 0.01, "truth {truth} got {}", e.cm360);
        }
    }

    #[test]
    fn every_round_halves_log_width() {
        let mut s = FeelSession::new(FeelConfig::default()).unwrap();
        let w0 = (s.hi / s.lo).ln();
        s.submit(Choice::Same).unwrap();
        let w1 = (s.hi / s.lo).ln();
        assert!((w1 / w0 - 0.5).abs() < 1e-9);
        s.submit(Choice::A).unwrap();
        assert!(((s.hi / s.lo).ln() / w1 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn stops_at_round_cap() {
        let mut s = FeelSession::new(FeelConfig { tolerance_pct: 0.0001, max_rounds: 3, ..Default::default() }).unwrap();
        for _ in 0..3 {
            s.submit(Choice::B).unwrap();
        }
        assert!(s.done);
        assert!(s.submit(Choice::A).is_err());
        assert_eq!(s.estimate().rounds_used, 3);
    }

    #[test]
    fn undo_restores_range() {
        let mut s = FeelSession::new(FeelConfig::default()).unwrap();
        let (lo, hi) = (s.lo, s.hi);
        s.submit(Choice::A).unwrap();
        assert!(s.undo());
        assert_eq!((s.lo, s.hi), (lo, hi));
        assert!(s.current().is_some());
    }

    #[test]
    fn suggestion_uses_scores() {
        let mut s = FeelSession::new(FeelConfig::default()).unwrap();
        let good = FlickScore { accuracy: 0.95, avg_ttk_ms: 420.0, overshoot_pct: 4.0 };
        let bad = FlickScore { accuracy: 0.82, avg_ttk_ms: 510.0, overshoot_pct: 12.0 };
        s.record_scores(Some(bad), Some(good));
        assert_eq!(s.suggestion(), Some(Choice::B));
        s.record_scores(Some(good), None);
        assert_eq!(s.suggestion(), Some(Choice::Same));
    }

    #[test]
    fn rejects_bad_config() {
        assert!(FeelSession::new(FeelConfig { base_cm360: 0.0, ..Default::default() }).is_err());
        assert!(FeelSession::new(FeelConfig { spread: 0.95, ..Default::default() }).is_err());
    }
}
