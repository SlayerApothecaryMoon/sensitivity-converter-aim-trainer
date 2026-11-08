//! Game catalog.
//!
//! Every supported game is described by its *yaw* (degrees of camera rotation per
//! mouse count at an in-game sensitivity of 1.0), the way its FOV slider is measured,
//! and how its sensitivity is written to a config file. The conversion engine only
//! ever talks to this table, so adding a game is a single entry.

use serde::Serialize;

/// How a game's FOV value is defined. Every model assumes Hor+ scaling, which
/// means the vertical FOV stays constant when the aspect ratio changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FovKind {
    /// Horizontal FOV measured at a 16:9 reference aspect.
    Hor16x9,
    /// Horizontal FOV measured at a 4:3 reference aspect (Source / IW style).
    Hor4x3,
    /// Vertical FOV, independent of aspect ratio.
    Vertical,
}

impl FovKind {
    /// Aspect ratio the FOV value refers to (`None` for vertical).
    pub fn reference_aspect(self) -> Option<f64> {
        match self {
            FovKind::Hor16x9 => Some(16.0 / 9.0),
            FovKind::Hor4x3 => Some(4.0 / 3.0),
            FovKind::Vertical => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FovKind::Hor16x9 => "hor 16:9",
            FovKind::Hor4x3 => "hor 4:3",
            FovKind::Vertical => "vertical",
        }
    }
}

/// Unit the user types into the game's sensitivity field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensUnit {
    /// Plain multiplier (Valorant, CS2, Overwatch ...).
    Multiplier,
    /// Percent slider (Fortnite shows 6.4 for a config value of 0.064).
    Percent,
    /// Integer slider (Rainbow Six Siege).
    Integer,
}

#[derive(Debug, Clone, Serialize)]
pub struct Game {
    pub id: &'static str,
    pub name: &'static str,
    /// Three letter code used on compact UI tiles.
    pub code: &'static str,
    pub engine: &'static str,
    /// Degrees per count at sensitivity 1.0 (in `unit`).
    pub yaw: f64,
    pub unit: SensUnit,
    pub fov_kind: FovKind,
    pub default_fov: f64,
    pub fov_min: f64,
    pub fov_max: f64,
    pub sens_min: f64,
    pub sens_max: f64,
    /// Decimals the game accepts in its sensitivity field.
    pub decimals: u8,
    /// Main executable, used to detect a running game.
    pub process: &'static str,
    /// `false` = yaw / FOV model still needs a measurement pass.
    pub verified: bool,
}

impl Game {
    pub fn fov_adjustable(&self) -> bool {
        (self.fov_max - self.fov_min).abs() > f64::EPSILON
    }

    /// Clamp a requested FOV into the game's slider range (default when absent).
    pub fn fov_or_default(&self, fov: Option<f64>) -> f64 {
        fov.unwrap_or(self.default_fov).clamp(self.fov_min, self.fov_max)
    }

    /// Vertical FOV in degrees for the given in-game FOV value.
    pub fn vertical_fov(&self, fov: f64) -> f64 {
        match self.fov_kind.reference_aspect() {
            Some(aspect) => vfov_from_hfov(fov, aspect),
            None => fov,
        }
    }

    /// Real horizontal FOV on a screen with `aspect` (width / height).
    pub fn horizontal_fov(&self, fov: f64, aspect: f64) -> f64 {
        hfov_from_vfov(self.vertical_fov(fov), aspect)
    }

    /// Round a sensitivity to what the game's input field can store.
    pub fn round_sens(&self, sens: f64) -> f64 {
        let p = 10f64.powi(self.decimals as i32);
        ((sens * p).round() / p).clamp(self.sens_min, self.sens_max)
    }
}

pub fn hfov_from_vfov(vfov_deg: f64, aspect: f64) -> f64 {
    let half = (vfov_deg.to_radians() / 2.0).tan() * aspect;
    2.0 * half.atan().to_degrees()
}

pub fn vfov_from_hfov(hfov_deg: f64, aspect: f64) -> f64 {
    let half = (hfov_deg.to_radians() / 2.0).tan() / aspect;
    2.0 * half.atan().to_degrees()
}

// TODO(owner): re-measure the entries marked `verified: false` with the 360 verify
// tool after each major game patch and keep a changelog of yaw changes.
pub static GAMES: &[Game] = &[
    Game { id: "valorant", name: "Valorant", code: "VAL", engine: "Unreal Engine 4", yaw: 0.07,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor16x9, default_fov: 103.0, fov_min: 103.0, fov_max: 103.0,
        sens_min: 0.001, sens_max: 10.0, decimals: 3, process: "VALORANT-Win64-Shipping.exe", verified: true },
    Game { id: "cs2", name: "Counter-Strike 2", code: "CS2", engine: "Source 2", yaw: 0.022,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor4x3, default_fov: 90.0, fov_min: 90.0, fov_max: 90.0,
        sens_min: 0.001, sens_max: 8.0, decimals: 3, process: "cs2.exe", verified: true },
    Game { id: "apex", name: "Apex Legends", code: "APX", engine: "Source (modified)", yaw: 0.022,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor4x3, default_fov: 90.0, fov_min: 70.0, fov_max: 110.0,
        sens_min: 0.1, sens_max: 20.0, decimals: 3, process: "r5apex.exe", verified: true },
    Game { id: "fortnite", name: "Fortnite", code: "FN", engine: "Unreal Engine 5", yaw: 0.005555,
        unit: SensUnit::Percent, fov_kind: FovKind::Hor16x9, default_fov: 80.0, fov_min: 80.0, fov_max: 80.0,
        sens_min: 0.1, sens_max: 100.0, decimals: 1, process: "FortniteClient-Win64-Shipping.exe", verified: true },
    Game { id: "overwatch2", name: "Overwatch 2", code: "OW2", engine: "Overwatch Engine", yaw: 0.0066,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor16x9, default_fov: 103.0, fov_min: 80.0, fov_max: 103.0,
        sens_min: 0.01, sens_max: 100.0, decimals: 2, process: "Overwatch.exe", verified: true },
    Game { id: "r6s", name: "Rainbow Six Siege", code: "R6S", engine: "AnvilNext 2.0", yaw: 0.005_729_578,
        unit: SensUnit::Integer, fov_kind: FovKind::Vertical, default_fov: 60.0, fov_min: 60.0, fov_max: 90.0,
        sens_min: 1.0, sens_max: 100.0, decimals: 0, process: "RainbowSix.exe", verified: true },
    Game { id: "cod", name: "Call of Duty", code: "COD", engine: "IW 9", yaw: 0.0066,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor4x3, default_fov: 80.0, fov_min: 60.0, fov_max: 120.0,
        sens_min: 0.01, sens_max: 100.0, decimals: 2, process: "cod.exe", verified: true },
    Game { id: "the_finals", name: "The Finals", code: "FIN", engine: "Unreal Engine 5", yaw: 0.0066,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Vertical, default_fov: 71.0, fov_min: 60.0, fov_max: 100.0,
        sens_min: 0.01, sens_max: 100.0, decimals: 2, process: "Discovery.exe", verified: false },
    Game { id: "deadlock", name: "Deadlock", code: "DLK", engine: "Source 2", yaw: 0.044,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor4x3, default_fov: 90.0, fov_min: 90.0, fov_max: 90.0,
        sens_min: 0.01, sens_max: 8.0, decimals: 3, process: "project8.exe", verified: false },
    Game { id: "marvel_rivals", name: "Marvel Rivals", code: "MR", engine: "Unreal Engine 5", yaw: 0.0175,
        unit: SensUnit::Multiplier, fov_kind: FovKind::Hor16x9, default_fov: 90.0, fov_min: 90.0, fov_max: 90.0,
        sens_min: 0.01, sens_max: 20.0, decimals: 2, process: "Marvel-Win64-Shipping.exe", verified: false },
];

pub fn find(id: &str) -> Option<&'static Game> {
    GAMES.iter().find(|g| g.id.eq_ignore_ascii_case(id))
}

/// Match a running process name to a game (case-insensitive).
pub fn by_process(process: &str) -> Option<&'static Game> {
    GAMES.iter().find(|g| g.process.eq_ignore_ascii_case(process))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_lookup_works() {
        let mut ids: Vec<_> = GAMES.iter().map(|g| g.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), GAMES.len());
        assert_eq!(find("CS2").unwrap().yaw, 0.022);
        assert!(by_process("valorant-win64-shipping.exe").is_some());
    }

    #[test]
    fn cs2_fov_is_106_at_16_9() {
        let cs = find("cs2").unwrap();
        let h = cs.horizontal_fov(90.0, 16.0 / 9.0);
        assert!((h - 106.26).abs() < 0.01, "{h}");
        assert!((cs.vertical_fov(90.0) - 73.74).abs() < 0.01);
    }

    #[test]
    fn fov_round_trip() {
        let v = vfov_from_hfov(103.0, 16.0 / 9.0);
        assert!((hfov_from_vfov(v, 16.0 / 9.0) - 103.0).abs() < 1e-9);
    }

    #[test]
    fn rounding_respects_decimals_and_range() {
        let r6 = find("r6s").unwrap();
        assert_eq!(r6.round_sens(11.6), 12.0);
        assert_eq!(r6.round_sens(0.2), 1.0);
        assert_eq!(find("valorant").unwrap().round_sens(0.41234), 0.412);
    }
}
