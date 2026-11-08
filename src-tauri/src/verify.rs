//! 360 verify: check that a known number of mouse counts really turns 360° in game,
//! and that the mouse reports the DPI printed on the box.
//!
//! The math is pure and tested. Capturing raw counts needs a WM_INPUT listener.

use crate::convert::{counts_per_360, CM_PER_INCH};
use crate::error::{AppError, AppResult};
use crate::games;
use serde::Serialize;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

#[derive(Debug, Clone, Serialize)]
pub struct VerifyPlan {
    pub game: &'static str,
    pub sens: f64,
    /// Counts that must be sent for exactly one full turn.
    pub counts: f64,
    pub cm: f64,
    /// Fractional count left over; non-zero values accumulate drift in some engines.
    pub remainder: f64,
}

/// What the user observed after sending `counts` (expected 360°).
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub expected_deg: f64,
    pub observed_deg: f64,
    pub error_pct: f64,
    /// Yaw implied by the observation; feed back into the game table if consistent.
    pub implied_yaw: f64,
    pub ok: bool,
}

pub fn plan(game_id: &str, sens: f64, dpi: u32) -> AppResult<VerifyPlan> {
    let g = games::find(game_id).ok_or_else(|| AppError::UnknownGame(game_id.into()))?;
    if sens <= 0.0 || dpi == 0 {
        return Err(AppError::Invalid("sensitivity and DPI must be positive".into()));
    }
    let counts = counts_per_360(g.yaw, sens);
    Ok(VerifyPlan { game: g.id, sens, counts, cm: counts / dpi as f64 * CM_PER_INCH, remainder: counts - counts.floor() })
}

/// `overshoot_deg` is how far past (positive) or short of (negative) the start the camera ended.
pub fn evaluate(game_id: &str, sens: f64, counts_sent: f64, overshoot_deg: f64) -> AppResult<VerifyResult> {
    let g = games::find(game_id).ok_or_else(|| AppError::UnknownGame(game_id.into()))?;
    let expected = counts_sent * g.yaw * sens;
    let observed = expected + overshoot_deg;
    let error_pct = (observed / expected - 1.0) * 100.0;
    Ok(VerifyResult {
        expected_deg: expected,
        observed_deg: observed,
        error_pct,
        implied_yaw: observed / (counts_sent * sens),
        ok: error_pct.abs() < 0.25,
    })
}

/// Real DPI from a measured stroke: the user drags exactly `cm` along a ruler.
pub fn measured_dpi(counts: f64, cm: f64) -> f64 {
    counts / (cm / CM_PER_INCH)
}

#[derive(Debug, Clone, Serialize)]
pub struct MouseSettings {
    /// "Enhance pointer precision" (mouse acceleration) on the desktop.
    pub enhance_pointer_precision: bool,
    /// Windows pointer speed slider, 1..20 (10 = 6/11, the neutral notch).
    pub pointer_speed: u32,
    pub pointer_multiplier: f64,
    pub screen_w: i32,
    pub screen_h: i32,
    pub aspect: f64,
    pub warnings: Vec<String>,
}

/// Windows pointer speed multipliers per slider notch (1..20).
const SPEED_TABLE: [f64; 20] = [
    0.03125, 0.0625, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5,
];

pub fn pointer_multiplier(notch: u32) -> f64 {
    SPEED_TABLE[(notch.clamp(1, 20) - 1) as usize]
}

fn screen_size() -> (i32, i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    // SAFETY: GetSystemMetrics has no preconditions and only reads system state.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

pub fn mouse_settings() -> MouseSettings {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Control Panel\\Mouse").ok();
    let read = |name: &str| key.as_ref().and_then(|k| k.get_value::<String, _>(name).ok());
    let epp = read("MouseSpeed").map(|v| v.trim() != "0").unwrap_or(false);
    let speed = read("MouseSensitivity").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(10);
    let (w, h) = screen_size();
    let aspect = if w > 0 && h > 0 { w as f64 / h as f64 } else { 16.0 / 9.0 };
    let mut warnings = Vec::new();
    if epp {
        warnings.push("Enhance pointer precision is on. Games with raw input ignore it, the desktop and some menus do not.".into());
    }
    if speed != 10 {
        warnings.push(format!("Windows pointer speed is {speed}/20; games without raw input scale counts by {}x.", pointer_multiplier(speed)));
    }
    MouseSettings {
        enhance_pointer_precision: epp,
        pointer_speed: speed,
        pointer_multiplier: pointer_multiplier(speed),
        screen_w: w,
        screen_h: h,
        aspect,
        warnings,
    }
}

/// Raw mouse count accumulator for the 360 verify and DPI check screens.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RawCounter {
    pub dx: i64,
    pub dy: i64,
    pub armed: bool,
}

impl RawCounter {
    pub fn arm(&mut self) {
        *self = Self { armed: true, ..Default::default() };
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn feed(&mut self, dx: i32, dy: i32) {
        if self.armed {
            self.dx += dx as i64;
            self.dy += dy as i64;
        }
    }

    // TODO(owner): register a RAWINPUTDEVICE (usage page 0x01, usage 0x02, RIDEV_INPUTSINK)
    // on a hidden message-only window and call `feed` from WM_INPUT. Needs the
    // Win32_UI_Input feature, which is intentionally not enabled in this template.
    // Read-only: SensMatch never sends input to a game.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_for_valorant() {
        let p = plan("valorant", 0.4, 800).unwrap();
        assert!((p.counts - 12857.142_857).abs() < 1e-3);
        assert!((p.cm - 40.821).abs() < 0.01);
        assert!(p.remainder > 0.14 && p.remainder < 0.15);
    }

    #[test]
    fn evaluate_detects_wrong_yaw() {
        let ok = evaluate("cs2", 2.0, 8181.818, 0.0).unwrap();
        assert!(ok.ok && (ok.implied_yaw - 0.022).abs() < 1e-12);
        let off = evaluate("cs2", 2.0, 8181.818, 9.0).unwrap();
        assert!(!off.ok && (off.error_pct - 2.5).abs() < 0.01);
    }

    #[test]
    fn dpi_measurement() {
        assert!((measured_dpi(3150.0, 10.0) - 800.1).abs() < 0.01);
    }

    #[test]
    fn pointer_table() {
        assert_eq!(pointer_multiplier(10), 1.0);
        assert_eq!(pointer_multiplier(0), 0.03125);
        assert_eq!(pointer_multiplier(99), 3.5);
    }

    #[test]
    fn raw_counter_only_counts_when_armed() {
        let mut c = RawCounter::default();
        c.feed(10, 0);
        assert_eq!(c.dx, 0);
        c.arm();
        c.feed(10, -3);
        c.feed(5, 1);
        assert_eq!((c.dx, c.dy), (15, -2));
    }
}
