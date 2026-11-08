//! Tauri command layer: thin wrappers that validate input and lock state.

use crate::aim::{self, AimConfig, AimEngine, AimReport, Sample, ShotResult, Target};
use crate::convert::{self, ConvertRequest, ConvertResult, EquivalentRow};
use crate::error::{AppError, AppResult};
use crate::feel::{Choice, FeelConfig, FeelSession, FlickScore};
use crate::games::{self, Game};
use crate::history::{SessionRecord, Trend};
use crate::profiles::{self, ConfigExport, Profile};
use crate::state::{lock, AppState};
use crate::verify::{self, MouseSettings, RawCounter, VerifyPlan, VerifyResult};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
pub struct GameInfo {
    #[serde(flatten)]
    pub game: Game,
    pub fov_adjustable: bool,
    pub fov_label: &'static str,
}

#[tauri::command]
pub fn list_games() -> Vec<GameInfo> {
    games::GAMES
        .iter()
        .map(|g| GameInfo { game: g.clone(), fov_adjustable: g.fov_adjustable(), fov_label: g.fov_kind.label() })
        .collect()
}

#[tauri::command]
pub fn convert(req: ConvertRequest) -> AppResult<ConvertResult> {
    convert::convert(&req)
}

#[tauri::command]
pub fn equivalents(cm360: f64, dpi: u32) -> AppResult<Vec<EquivalentRow>> {
    if !(cm360 > 0.0) {
        return Err(AppError::Invalid("cm/360 must be positive".into()));
    }
    Ok(convert::equivalents(cm360, dpi))
}

#[tauri::command]
pub fn feel_start(state: State<AppState>, config: Option<FeelConfig>) -> AppResult<FeelSession> {
    let s = FeelSession::new(config.unwrap_or_default())?;
    *lock(&state.feel) = Some(s.clone());
    Ok(s)
}

#[tauri::command]
pub fn feel_submit(state: State<AppState>, choice: Choice) -> AppResult<FeelSession> {
    let mut g = lock(&state.feel);
    let s = g.as_mut().ok_or(AppError::NoSession("feel match"))?;
    s.submit(choice)?;
    Ok(s.clone())
}

#[tauri::command]
pub fn feel_scores(state: State<AppState>, a: Option<FlickScore>, b: Option<FlickScore>) -> AppResult<Option<Choice>> {
    let mut g = lock(&state.feel);
    let s = g.as_mut().ok_or(AppError::NoSession("feel match"))?;
    s.record_scores(a, b);
    Ok(s.suggestion())
}

#[tauri::command]
pub fn feel_undo(state: State<AppState>) -> AppResult<FeelSession> {
    let mut g = lock(&state.feel);
    let s = g.as_mut().ok_or(AppError::NoSession("feel match"))?;
    s.undo();
    Ok(s.clone())
}

#[tauri::command]
pub fn aim_start(state: State<AppState>, config: Option<AimConfig>) -> Vec<Target> {
    let mut cfg = config.unwrap_or_default();
    if cfg.seed == 0 {
        cfg.seed = profiles::now_unix();
    }
    cfg.radius = cfg.radius.clamp(6.0, 60.0);
    let mut eng = AimEngine::new(cfg);
    let first = eng.start();
    *lock(&state.aim) = Some(eng);
    first
}

#[tauri::command]
pub fn aim_track(state: State<AppState>, samples: Vec<Sample>) -> AppResult<()> {
    lock(&state.aim).as_mut().ok_or(AppError::NoSession("aim test"))?.track(&samples);
    Ok(())
}

#[tauri::command]
pub fn aim_shoot(state: State<AppState>, t: f64, x: f64, y: f64) -> AppResult<ShotResult> {
    let mut g = lock(&state.aim);
    let eng = g.as_mut().ok_or(AppError::NoSession("aim test"))?;
    // small grace period for the shot that lands on the final frame
    if t > eng.config().duration_ms + 1500.0 {
        return Err(AppError::Invalid("session time is over".into()));
    }
    Ok(eng.shoot(t, x, y))
}

/// What the UI knows about the sensitivity the session was played at.
#[derive(Debug, Deserialize)]
pub struct SessionMeta {
    pub game: String,
    pub sens: f64,
    pub cm360: f64,
}

#[tauri::command]
pub fn aim_finish(state: State<AppState>, meta: Option<SessionMeta>) -> AppResult<AimReport> {
    let eng = lock(&state.aim).take().ok_or(AppError::NoSession("aim test"))?;
    let report = eng.report();
    // sessions with fewer than 5 kills are aborted runs, not worth keeping
    if let Some(m) = meta.filter(|_| report.live.hits >= 5) {
        lock(&state.history).add(SessionRecord::from_report(&report, &m.game, m.sens, m.cm360))?;
    }
    *lock(&state.last_report) = Some(report.clone());
    Ok(report)
}

/// Last finished session, or a simulated one so the results screen has content.
#[tauri::command]
pub fn aim_report(state: State<AppState>) -> AimReport {
    lock(&state.last_report).clone().unwrap_or_else(|| aim::simulate(AimConfig::default(), 0.82))
}

#[tauri::command]
pub fn sessions_list(state: State<AppState>, limit: Option<usize>) -> Vec<SessionRecord> {
    lock(&state.history).recent(limit.unwrap_or(20).min(200))
}

#[tauri::command]
pub fn history_trend(state: State<AppState>) -> Trend {
    lock(&state.history).trend(profiles::now_unix())
}

#[tauri::command]
pub fn verify_plan(game: String, sens: f64, dpi: u32) -> AppResult<VerifyPlan> {
    verify::plan(&game, sens, dpi)
}

#[tauri::command]
pub fn verify_evaluate(game: String, sens: f64, counts: f64, overshoot_deg: f64) -> AppResult<VerifyResult> {
    verify::evaluate(&game, sens, counts, overshoot_deg)
}

#[tauri::command]
pub fn verify_dpi(counts: f64, cm: f64) -> AppResult<f64> {
    if !(cm > 0.5 && counts > 0.0) {
        return Err(AppError::Invalid("move at least 0.5 cm".into()));
    }
    Ok(verify::measured_dpi(counts, cm))
}

/// Start counting raw mouse counts for 360 verify / DPI check.
#[tauri::command]
pub fn raw_arm(state: State<AppState>) {
    lock(&state.raw).arm();
}

#[tauri::command]
pub fn raw_read(state: State<AppState>) -> RawCounter {
    lock(&state.raw).clone()
}

#[derive(Serialize)]
pub struct SystemStatus {
    pub mouse: MouseSettings,
    pub running_game: Option<&'static str>,
    pub version: &'static str,
}

#[tauri::command]
pub fn system_status(state: State<AppState>) -> SystemStatus {
    SystemStatus { mouse: verify::mouse_settings(), running_game: *lock(&state.running_game), version: env!("CARGO_PKG_VERSION") }
}

#[tauri::command]
pub fn profiles_list(state: State<AppState>) -> Vec<Profile> {
    lock(&state.profiles).items.clone()
}

#[tauri::command]
pub fn profile_save(state: State<AppState>, profile: Profile) -> AppResult<Profile> {
    lock(&state.profiles).add(profile)
}

#[tauri::command]
pub fn profile_delete(state: State<AppState>, id: u64) -> AppResult<()> {
    lock(&state.profiles).remove(id)
}

#[tauri::command]
pub fn export_config(game: String, sens: f64, fov: Option<f64>) -> AppResult<ConfigExport> {
    profiles::export(&game, sens, fov)
}

#[tauri::command]
pub fn import_cs2() -> AppResult<f64> {
    profiles::import_cs2()
}
