//! Conversion engine: cm/360, eDPI and monitor distance matching.
//!
//! Notation: `yaw` = degrees per count at sensitivity 1.0, so one mouse count turns
//! the camera by `yaw * sens` degrees. Monitor distance percentages are measured on
//! the vertical axis (MDV): 100% = the top edge of the screen, 0% = the crosshair
//! itself (the limit case, equal to a focal-length / zoom-ratio match).

use crate::error::AppError;
use crate::games::{self, Game};
use serde::{Deserialize, Serialize};

pub const CM_PER_INCH: f64 = 2.54;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchMethod {
    /// Same physical distance for a full 360° turn. Ignores FOV.
    Distance360,
    /// Same mouse distance to move the crosshair onto a point that sits at
    /// `percent` of the vertical half-height of the screen.
    Monitor { percent: f64 },
}

impl MatchMethod {
    pub fn label(&self) -> String {
        match self {
            MatchMethod::Distance360 => "360° distance".into(),
            MatchMethod::Monitor { percent } => format!("MDV {}%", trim_float(*percent)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvertRequest {
    pub from_game: String,
    pub to_game: String,
    pub sens: f64,
    pub dpi: u32,
    /// Target DPI, defaults to the source DPI.
    pub to_dpi: Option<u32>,
    pub from_fov: Option<f64>,
    pub to_fov: Option<f64>,
    pub method: MatchMethod,
    /// Screen aspect ratio (width / height); 16:9 when absent.
    pub aspect: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvertResult {
    pub from_game: &'static str,
    pub to_game: &'static str,
    pub method: String,
    pub sens_exact: f64,
    pub sens: f64,
    pub rounding_error_pct: f64,
    pub cm360_from: f64,
    pub cm360_to: f64,
    pub in360_to: f64,
    pub counts360_to: f64,
    pub edpi_from: f64,
    pub edpi_to: f64,
    pub hfov_from: f64,
    pub hfov_to: f64,
    pub vfov_from: f64,
    pub vfov_to: f64,
    /// Multiplier applied on top of the pure yaw ratio (1.0 for 360° distance).
    pub fov_ratio: f64,
    pub notes: Vec<String>,
}

pub fn deg_per_count(yaw: f64, sens: f64) -> f64 {
    yaw * sens
}

pub fn counts_per_360(yaw: f64, sens: f64) -> f64 {
    360.0 / deg_per_count(yaw, sens)
}

pub fn cm_per_360(yaw: f64, sens: f64, dpi: f64) -> f64 {
    counts_per_360(yaw, sens) / dpi * CM_PER_INCH
}

/// Inverse of [`cm_per_360`].
pub fn sens_for_cm360(yaw: f64, dpi: f64, cm360: f64) -> f64 {
    360.0 * CM_PER_INCH / (cm360 * dpi * yaw)
}

/// Classic eDPI (in-game sens x DPI, in the game's own units).
pub fn edpi(sens: f64, dpi: f64) -> f64 {
    sens * dpi
}

/// Ratio `deg_per_count_to / deg_per_count_from` that keeps the same mouse
/// distance for reaching a point at `percent` MDV.
pub fn monitor_ratio(vfov_from: f64, vfov_to: f64, percent: f64) -> f64 {
    let tf = (vfov_from.to_radians() / 2.0).tan();
    let tt = (vfov_to.to_radians() / 2.0).tan();
    if percent <= 1e-6 {
        // limit p -> 0 of atan(p*a)/atan(p*b) = a/b
        return tt / tf;
    }
    let p = percent / 100.0;
    (p * tt).atan() / (p * tf).atan()
}

fn game(id: &str) -> Result<&'static Game, AppError> {
    games::find(id).ok_or_else(|| AppError::UnknownGame(id.to_string()))
}

pub fn convert(req: &ConvertRequest) -> Result<ConvertResult, AppError> {
    if !(req.sens.is_finite() && req.sens > 0.0) {
        return Err(AppError::Invalid("sensitivity must be a positive number".into()));
    }
    if req.dpi == 0 || req.dpi > 64_000 {
        return Err(AppError::Invalid("DPI must be between 1 and 64000".into()));
    }
    let from = game(&req.from_game)?;
    let to = game(&req.to_game)?;
    let aspect = req.aspect.filter(|a| *a > 0.5 && *a < 5.0).unwrap_or(16.0 / 9.0);
    let dpi_from = req.dpi as f64;
    let dpi_to = req.to_dpi.filter(|d| *d > 0).unwrap_or(req.dpi) as f64;

    let fov_from = from.fov_or_default(req.from_fov);
    let fov_to = to.fov_or_default(req.to_fov);
    let (vf, vt) = (from.vertical_fov(fov_from), to.vertical_fov(fov_to));

    let fov_ratio = match req.method {
        MatchMethod::Distance360 => 1.0,
        MatchMethod::Monitor { percent } => monitor_ratio(vf, vt, percent.clamp(0.0, 400.0)),
    };

    // degrees per inch of mouse travel must scale by fov_ratio
    let deg_inch_from = deg_per_count(from.yaw, req.sens) * dpi_from;
    let sens_exact = deg_inch_from * fov_ratio / (to.yaw * dpi_to);
    let sens = to.round_sens(sens_exact);
    let rounding_error_pct = (sens / sens_exact - 1.0) * 100.0;

    let mut notes = Vec::new();
    if sens_exact < to.sens_min || sens_exact > to.sens_max {
        notes.push(format!(
            "{} only accepts {}..{}; try a different DPI.",
            to.name,
            trim_float(to.sens_min),
            trim_float(to.sens_max)
        ));
    }
    if rounding_error_pct.abs() > 0.5 {
        notes.push(format!("Rounded by {:+.2}% because {} stores {} decimals.", rounding_error_pct, to.name, to.decimals));
    }
    if !to.verified {
        notes.push(format!("{} values are provisional until re-measured with 360 verify.", to.name));
    }

    Ok(ConvertResult {
        from_game: from.id,
        to_game: to.id,
        method: req.method.label(),
        sens_exact,
        sens,
        rounding_error_pct,
        cm360_from: cm_per_360(from.yaw, req.sens, dpi_from),
        cm360_to: cm_per_360(to.yaw, sens, dpi_to),
        in360_to: cm_per_360(to.yaw, sens, dpi_to) / CM_PER_INCH,
        counts360_to: counts_per_360(to.yaw, sens),
        edpi_from: edpi(req.sens, dpi_from),
        edpi_to: edpi(sens, dpi_to),
        hfov_from: from.horizontal_fov(fov_from, aspect),
        hfov_to: to.horizontal_fov(fov_to, aspect),
        vfov_from: vf,
        vfov_to: vt,
        fov_ratio,
        notes,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EquivalentRow {
    pub game: &'static str,
    pub name: &'static str,
    pub sens: f64,
    pub edpi: f64,
}

/// Express one physical cm/360 in every supported game (default FOVs, 360° match).
pub fn equivalents(cm360: f64, dpi: u32) -> Vec<EquivalentRow> {
    let d = dpi.max(1) as f64;
    games::GAMES
        .iter()
        .map(|g| {
            let sens = g.round_sens(sens_for_cm360(g.yaw, d, cm360));
            EquivalentRow { game: g.id, name: g.name, sens, edpi: edpi(sens, d) }
        })
        .collect()
}

pub fn trim_float(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(from: &str, to: &str, sens: f64, method: MatchMethod) -> ConvertRequest {
        ConvertRequest {
            from_game: from.into(), to_game: to.into(), sens, dpi: 800, to_dpi: None,
            from_fov: None, to_fov: None, method, aspect: None,
        }
    }

    #[test]
    fn valorant_to_cs2_360() {
        let r = convert(&req("valorant", "cs2", 0.4, MatchMethod::Distance360)).unwrap();
        assert!((r.sens_exact - 1.272_727).abs() < 1e-5, "{}", r.sens_exact);
        assert_eq!(r.sens, 1.273);
        assert!((r.cm360_from - 40.821).abs() < 0.01, "{}", r.cm360_from);
    }

    #[test]
    fn cm360_round_trip() {
        let s = sens_for_cm360(0.022, 1600.0, 34.6);
        assert!((cm_per_360(0.022, s, 1600.0) - 34.6).abs() < 1e-9);
    }

    #[test]
    fn doubling_dpi_halves_sens() {
        let mut r = req("cs2", "apex", 2.0, MatchMethod::Distance360);
        r.to_dpi = Some(1600);
        let out = convert(&r).unwrap();
        assert!((out.sens_exact - 1.0).abs() < 1e-12);
    }

    #[test]
    fn equal_fov_monitor_match_equals_360() {
        for p in [0.0, 56.25, 75.0, 100.0] {
            let r = convert(&req("cs2", "apex", 1.5, MatchMethod::Monitor { percent: p })).unwrap();
            assert!((r.fov_ratio - 1.0).abs() < 1e-12);
            assert!((r.sens_exact - 1.5).abs() < 1e-12);
        }
    }

    #[test]
    fn mdv_zero_is_zoom_ratio() {
        let r = monitor_ratio(60.0, 90.0, 0.0);
        let expect = (45f64.to_radians()).tan() / (30f64.to_radians()).tan();
        assert!((r - expect).abs() < 1e-12);
    }

    #[test]
    fn monitor_ratio_moves_towards_one_as_percent_grows() {
        let mut last = monitor_ratio(73.74, 70.53, 0.0);
        for p in [25.0, 56.25, 75.0, 100.0, 177.78, 400.0] {
            let r = monitor_ratio(73.74, 70.53, p);
            assert!(r > last && r < 1.0, "p={p} r={r} last={last}");
            last = r;
        }
    }

    #[test]
    fn overwatch_to_valorant_mdv_differs_from_360() {
        let a = convert(&req("overwatch2", "valorant", 5.0, MatchMethod::Distance360)).unwrap();
        let b = convert(&req("overwatch2", "valorant", 5.0, MatchMethod::Monitor { percent: 0.0 })).unwrap();
        assert!((a.sens_exact - 0.471_428).abs() < 1e-5);
        // same 103° hor 16:9 -> identical FOV -> identical result
        assert!((a.sens_exact - b.sens_exact).abs() < 1e-12);
        let c = convert(&req("cs2", "valorant", 1.5, MatchMethod::Monitor { percent: 0.0 })).unwrap();
        assert!(c.fov_ratio < 1.0);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(convert(&req("valorant", "cs2", 0.0, MatchMethod::Distance360)).is_err());
        assert!(convert(&req("valorant", "nope", 1.0, MatchMethod::Distance360)).is_err());
    }

    #[test]
    fn equivalents_cover_all_games() {
        let rows = equivalents(40.82, 800);
        assert_eq!(rows.len(), games::GAMES.len());
        let val = rows.iter().find(|r| r.game == "valorant").unwrap();
        assert!((val.sens - 0.4).abs() < 0.002);
    }

    #[test]
    fn trims_floats() {
        assert_eq!(trim_float(56.25), "56.25");
        assert_eq!(trim_float(100.0), "100");
    }
}
