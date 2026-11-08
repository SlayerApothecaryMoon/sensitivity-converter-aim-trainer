//! Profile history, config export lines and importing the current CS2 sensitivity.
//!
//! Profiles live in `%APPDATA%\SensMatch\profiles.json` (plain JSON, human-editable).

use crate::convert::{cm_per_360, trim_float};
use crate::error::{AppError, AppResult};
use crate::games::{self, Game};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PROFILES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub created: u64,
    pub game: String,
    pub sens: f64,
    pub dpi: u32,
    pub fov: Option<f64>,
    pub cm360: f64,
    /// "convert", "feel" or "import".
    pub source: String,
    pub note: String,
}

#[derive(Debug, Default)]
pub struct ProfileStore {
    path: Option<PathBuf>,
    pub items: Vec<Profile>,
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("SensMatch").join("profiles.json"))
}

impl ProfileStore {
    /// Load from disk; seed sample history on first run so the UI is never empty.
    pub fn open(path: Option<PathBuf>) -> Self {
        let items = path
            .as_deref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Vec<Profile>>(&s).ok())
            .unwrap_or_else(sample_history);
        Self { path, items }
    }

    pub fn save(&self) -> AppResult<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.items)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn add(&mut self, mut p: Profile) -> AppResult<Profile> {
        let g = games::find(&p.game).ok_or_else(|| AppError::UnknownGame(p.game.clone()))?;
        if p.sens <= 0.0 || p.dpi == 0 {
            return Err(AppError::Invalid("profile needs a positive sensitivity and DPI".into()));
        }
        p.id = self.items.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        p.created = if p.created == 0 { now_unix() } else { p.created };
        p.cm360 = cm_per_360(g.yaw, p.sens, p.dpi as f64);
        if p.name.trim().is_empty() {
            p.name = format!("{} {} @ {}", g.name, trim_float(p.sens), p.dpi);
        }
        // the same setup saved twice only bumps its timestamp
        self.items.retain(|x| !(x.game == p.game && x.dpi == p.dpi && (x.sens - p.sens).abs() < 1e-9));
        self.items.insert(0, p.clone());
        self.items.truncate(MAX_PROFILES);
        self.save()?;
        Ok(p)
    }

    pub fn remove(&mut self, id: u64) -> AppResult<()> {
        let before = self.items.len();
        self.items.retain(|p| p.id != id);
        if self.items.len() == before {
            return Err(AppError::NotFound(format!("profile {id}")));
        }
        self.save()
    }
}

fn sample_history() -> Vec<Profile> {
    let base = now_unix();
    let rows: [(&str, &str, f64, u32, &str, u64, &str); 6] = [
        ("Main - tactical", "valorant", 0.312, 1600, "feel", 3_600, "Feel Match, 7 rounds, +/-1.1%"),
        ("CS2 from Valorant", "cs2", 0.993, 1600, "convert", 26_000, "360° distance"),
        ("Apex hipfire", "apex", 1.6, 800, "convert", 90_000, "MDV 0%, FOV 104"),
        ("Overwatch tracking", "overwatch2", 4.73, 800, "convert", 260_000, "MDV 56.25%"),
        ("CS2 autoexec", "cs2", 1.25, 800, "import", 600_000, "Imported from cs2_user_convars"),
        ("Fortnite build", "fortnite", 6.4, 800, "convert", 1_400_000, "360° distance"),
    ];
    rows.iter()
        .enumerate()
        .map(|(i, (name, game, sens, dpi, src, age, note))| {
            let yaw = games::find(game).map(|g| g.yaw).unwrap_or(0.022);
            Profile {
                id: (rows.len() - i) as u64,
                name: name.to_string(),
                created: base.saturating_sub(*age),
                game: game.to_string(),
                sens: *sens,
                dpi: *dpi,
                fov: None,
                cm360: cm_per_360(yaw, *sens, *dpi as f64),
                source: src.to_string(),
                note: note.to_string(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigExport {
    pub game: &'static str,
    /// Where the line goes (file or settings path).
    pub target: &'static str,
    pub lines: Vec<String>,
    pub hint: &'static str,
}

fn fmt_sens(g: &Game, sens: f64) -> String {
    format!("{:.*}", g.decimals as usize, g.round_sens(sens))
}

/// Ready-to-paste config line(s) for a sensitivity in the given game.
pub fn export(game_id: &str, sens: f64, fov: Option<f64>) -> AppResult<ConfigExport> {
    let g = games::find(game_id).ok_or_else(|| AppError::UnknownGame(game_id.into()))?;
    let s = fmt_sens(g, sens);
    let fov = g.fov_or_default(fov);
    let (target, lines, hint): (&'static str, Vec<String>, &'static str) = match g.id {
        "cs2" => ("cfg\\autoexec.cfg", vec![format!("sensitivity \"{s}\""), "m_yaw \"0.022\"".into()], "Add +exec autoexec to launch options."),
        "deadlock" => ("cfg\\autoexec.cfg", vec![format!("sensitivity \"{s}\"")], "Or paste into the console (F7)."),
        "apex" => (
            "Saved Games\\Respawn\\Apex\\local\\settings.cfg",
            vec![format!("mouse_sensitivity \"{s}\""), format!("cl_fovScale \"{:.6}\"", fov / 70.0)],
            "Close the game first; make the file read-only to keep it.",
        ),
        "r6s" => (
            "Documents\\My Games\\Rainbow Six - Siege\\<id>\\GameSettings.ini",
            vec![format!("MouseYawSensitivity={s}"), format!("MousePitchSensitivity={s}"), "MouseSensitivityMultiplierUnit=0.020000".into(), format!("DefaultFOV={fov:.0}")],
            "Values assume the default 0.02 multiplier.",
        ),
        "fortnite" => ("Settings > Mouse & Keyboard", vec![format!("Mouse X Sensitivity: {s}%"), format!("Mouse Y Sensitivity: {s}%")], "Keep X and Y equal."),
        "valorant" => ("Settings > General > Mouse", vec![format!("Sensitivity: Aim {s}")], "Scoped multiplier 1.000 keeps the same feel."),
        "overwatch2" => ("Options > Controls > General", vec![format!("Sensitivity {s}"), format!("Field of view {fov:.0}")], "Hero specific overrides win over the global value."),
        "cod" => ("Settings > Controller / Mouse", vec![format!("Mouse Sensitivity {s}"), format!("Field of View {fov:.0}"), "ADS Sensitivity Multiplier 1.00".into()], "Use Relative ADS for monitor distance."),
        _ => ("In-game settings", vec![format!("Mouse sensitivity {s}")], "Provisional values; verify with 360 verify."),
    };
    Ok(ConfigExport { game: g.id, target, lines, hint })
}

/// Pull a quoted value out of a Valve KeyValues file (`"key"  "value"`).
pub fn parse_vcfg_value(text: &str, key: &str) -> Option<f64> {
    text.lines().find_map(|line| {
        let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
        let k = parts.next()?;
        if !k.eq_ignore_ascii_case(key) {
            return None;
        }
        parts.next()?.trim().parse::<f64>().ok()
    })
}

fn steam_path() -> Option<PathBuf> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam").ok()?;
    key.get_value::<String, _>("SteamPath").ok().map(PathBuf::from)
}

/// Newest `cs2_user_convars_0_slot0.vcfg` under `<steam>\userdata\*`.
fn newest_cs2_cfg(steam: &Path) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for user in fs::read_dir(steam.join("userdata")).ok()?.flatten() {
        let p = user.path().join("730\\local\\cfg\\cs2_user_convars_0_slot0.vcfg");
        if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().map_or(true, |(t, _)| m > *t) {
                best = Some((m, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Read the sensitivity CS2 is currently using on this PC.
pub fn import_cs2() -> AppResult<f64> {
    let steam = steam_path().ok_or_else(|| AppError::NotFound("Steam installation".into()))?;
    let cfg = newest_cs2_cfg(&steam).ok_or_else(|| AppError::NotFound("CS2 user config".into()))?;
    let text = fs::read_to_string(&cfg)?;
    parse_vcfg_value(&text, "sensitivity").ok_or_else(|| AppError::NotFound("sensitivity in CS2 config".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> ProfileStore {
        let p = std::env::temp_dir().join(format!("sensmatch-test-{tag}-{}.json", std::process::id()));
        let _ = fs::remove_file(&p);
        ProfileStore::open(Some(p))
    }

    #[test]
    fn vcfg_parsing() {
        let text = "\"config\"\n{\n\t\"convars\"\n\t{\n\t\t\"sensitivity\"\t\t\"1.250000\"\n\t\t\"zoom_sensitivity_ratio\"\t\t\"1.0\"\n\t}\n}";
        assert_eq!(parse_vcfg_value(text, "sensitivity"), Some(1.25));
        assert_eq!(parse_vcfg_value(text, "m_yaw"), None);
    }

    #[test]
    fn add_dedupes_and_persists() {
        let mut s = temp_store("add");
        let n = s.items.len();
        let p = Profile { id: 0, name: String::new(), created: 0, game: "cs2".into(), sens: 1.1, dpi: 800, fov: None, cm360: 0.0, source: "convert".into(), note: String::new() };
        let saved = s.add(p.clone()).unwrap();
        assert!((saved.cm360 - 47.231).abs() < 0.01);
        assert_eq!(saved.name, "Counter-Strike 2 1.1 @ 800");
        s.add(p).unwrap();
        assert_eq!(s.items.len(), n + 1);
        let reopened = ProfileStore::open(s.path.clone());
        assert_eq!(reopened.items, s.items);
        s.remove(saved.id + 1).unwrap();
        assert!(s.remove(9999).is_err());
        let _ = fs::remove_file(s.path.unwrap());
    }

    #[test]
    fn export_lines() {
        let cs = export("cs2", 1.2727, None).unwrap();
        assert_eq!(cs.lines[0], "sensitivity \"1.273\"");
        let r6 = export("r6s", 11.6, Some(84.0)).unwrap();
        assert!(r6.lines.contains(&"MouseYawSensitivity=12".to_string()));
        assert!(r6.lines.contains(&"DefaultFOV=84".to_string()));
        assert!(export("unknown", 1.0, None).is_err());
    }
}
