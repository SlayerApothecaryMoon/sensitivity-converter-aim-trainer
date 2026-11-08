//! Aim test engine: deterministic target scheduling and scoring.
//!
//! The web UI only renders. It sends cursor samples and shots, the engine decides
//! hits, spawns the next targets and keeps every statistic. Coordinates are in
//! arena pixels with the origin at the top-left corner.

use serde::{Deserialize, Serialize};

/// SplitMix64: tiny, fast, good enough for gameplay randomness. Same seed, same session.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Standard normal sample (Box-Muller).
    pub fn normal(&mut self, mean: f64, sd: f64) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        mean + sd * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AimMode {
    /// One target at a time, long flicks.
    Flick,
    /// One target at a time, short corrections near the last target.
    Microflick,
    /// Three targets alive at once.
    Grid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AimConfig {
    pub mode: AimMode,
    pub seed: u64,
    pub duration_ms: f64,
    pub arena_w: f64,
    pub arena_h: f64,
    pub radius: f64,
}

impl Default for AimConfig {
    fn default() -> Self {
        Self { mode: AimMode::Flick, seed: 0x5E45_3600, duration_ms: 60_000.0, arena_w: 1040.0, arena_h: 600.0, radius: 18.0 }
    }
}

impl AimConfig {
    fn spawn_band(&self) -> (f64, f64) {
        let diag = (self.arena_w.powi(2) + self.arena_h.powi(2)).sqrt();
        match self.mode {
            AimMode::Flick => (diag * 0.22, diag * 0.55),
            AimMode::Microflick => (self.radius * 3.0, self.radius * 9.0),
            AimMode::Grid => (self.radius * 4.0, diag * 0.4),
        }
    }

    fn alive(&self) -> usize {
        if self.mode == AimMode::Grid { 3 } else { 1 }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Target {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub spawn_ms: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Sample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Outcome {
    pub target_id: u32,
    pub reaction_ms: f64,
    pub ttk_ms: f64,
    pub flick_px: f64,
    pub overshoot_pct: f64,
    /// Flick direction in degrees (0 = right, 90 = down).
    pub angle_deg: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShotResult {
    pub hit: Option<u32>,
    pub spawned: Vec<Target>,
    pub live: LiveStats,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct LiveStats {
    pub shots: u32,
    pub hits: u32,
    pub accuracy: f64,
    pub avg_reaction_ms: f64,
    pub avg_ttk_ms: f64,
    pub overshoot_pct: f64,
    pub score: u32,
}

/// Square grid of shot offsets relative to the nearest target, normalised by radius.
#[derive(Debug, Clone, Serialize)]
pub struct Heatmap {
    pub size: usize,
    /// Offsets covered on each axis, in target radii (+/- extent).
    pub extent: f64,
    pub cells: Vec<u32>,
}

impl Heatmap {
    pub fn new(size: usize, extent: f64) -> Self {
        Self { size, extent, cells: vec![0; size * size] }
    }

    pub fn add(&mut self, dx: f64, dy: f64) {
        let n = self.size as f64;
        let cx = ((dx / self.extent + 1.0) / 2.0 * n).floor();
        let cy = ((dy / self.extent + 1.0) / 2.0 * n).floor();
        if cx >= 0.0 && cy >= 0.0 && cx < n && cy < n {
            self.cells[cy as usize * self.size + cx as usize] += 1;
        }
    }

    pub fn total(&self) -> u32 {
        self.cells.iter().sum()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AimReport {
    pub config: AimConfig,
    pub live: LiveStats,
    pub kills_per_min: f64,
    pub best_reaction_ms: f64,
    pub reaction_hist: Vec<u32>,
    /// Bin width of `reaction_hist`, starting at `hist_start_ms`.
    pub hist_bin_ms: f64,
    pub hist_start_ms: f64,
    /// Average overshoot (%) per 45° direction sector, starting at "right".
    pub overshoot_by_dir: [f64; 8],
    pub heatmap: Heatmap,
    /// Shots that landed inside the heatmap window.
    pub mapped_shots: u32,
    pub outcomes: Vec<Outcome>,
}

pub struct AimEngine {
    cfg: AimConfig,
    rng: Rng,
    next_id: u32,
    active: Vec<Target>,
    /// Cursor position when each active target appeared.
    origin: Vec<(u32, f64, f64)>,
    path: Vec<Sample>,
    outcomes: Vec<Outcome>,
    heatmap: Heatmap,
    shots: u32,
    last_center: (f64, f64),
    cursor: (f64, f64),
}

const MARGIN: f64 = 24.0;

impl AimEngine {
    pub fn new(cfg: AimConfig) -> Self {
        let center = (cfg.arena_w / 2.0, cfg.arena_h / 2.0);
        Self {
            rng: Rng::new(cfg.seed),
            cfg,
            next_id: 1,
            active: Vec::new(),
            origin: Vec::new(),
            path: Vec::new(),
            outcomes: Vec::new(),
            heatmap: Heatmap::new(17, 3.0),
            shots: 0,
            last_center: center,
            cursor: center,
        }
    }

    pub fn config(&self) -> &AimConfig {
        &self.cfg
    }

    pub fn active(&self) -> &[Target] {
        &self.active
    }

    /// Spawn the opening targets.
    pub fn start(&mut self) -> Vec<Target> {
        (0..self.cfg.alive()).map(|_| self.spawn(0.0)).collect()
    }

    fn spawn(&mut self, now: f64) -> Target {
        let (min_d, max_d) = self.cfg.spawn_band();
        let r = self.cfg.radius;
        let (lx, ly) = self.last_center;
        let (w, h) = (self.cfg.arena_w, self.cfg.arena_h);
        let mut best = (w / 2.0, h / 2.0);
        for attempt in 0..48 {
            let ang = self.rng.range(0.0, std::f64::consts::TAU);
            let d = self.rng.range(min_d, max_d);
            let (x, y) = (lx + ang.cos() * d, ly + ang.sin() * d);
            let inside = x > MARGIN + r && x < w - MARGIN - r && y > MARGIN + r && y < h - MARGIN - r;
            let clear = self.active.iter().all(|t| ((t.x - x).powi(2) + (t.y - y).powi(2)).sqrt() > r * 3.0);
            if inside && clear {
                best = (x, y);
                break;
            }
            if attempt == 47 {
                // fall back to a uniform point inside the arena
                best = (self.rng.range(MARGIN + r, w - MARGIN - r), self.rng.range(MARGIN + r, h - MARGIN - r));
            }
        }
        let t = Target { id: self.next_id, x: best.0, y: best.1, r, spawn_ms: now };
        self.next_id += 1;
        self.last_center = best;
        self.active.push(t);
        self.origin.push((t.id, self.cursor.0, self.cursor.1));
        t
    }

    /// Feed cursor samples (batched by the UI every animation frame).
    pub fn track(&mut self, samples: &[Sample]) {
        if let Some(s) = samples.last() {
            self.cursor = (s.x, s.y);
        }
        self.path.extend_from_slice(samples);
        if self.path.len() > 4096 {
            let cut = self.path.len() - 2048;
            self.path.drain(..cut);
        }
    }

    pub fn shoot(&mut self, t: f64, x: f64, y: f64) -> ShotResult {
        self.shots += 1;
        self.cursor = (x, y);
        let nearest = self
            .active
            .iter()
            .enumerate()
            .map(|(i, tg)| (i, ((tg.x - x).powi(2) + (tg.y - y).powi(2)).sqrt()))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let mut spawned = Vec::new();
        let mut hit = None;
        if let Some((i, dist)) = nearest {
            let tg = self.active[i];
            self.heatmap.add((x - tg.x) / tg.r, (y - tg.y) / tg.r);
            if dist <= tg.r {
                hit = Some(tg.id);
                let outcome = self.measure(&tg, t);
                self.outcomes.push(outcome);
                self.active.remove(i);
                self.origin.retain(|o| o.0 != tg.id);
                self.path.clear();
                if t < self.cfg.duration_ms {
                    spawned.push(self.spawn(t));
                }
            }
        }
        ShotResult { hit, spawned, live: self.live() }
    }

    /// Reaction, TTK and overshoot for a target that has just been killed.
    fn measure(&self, tg: &Target, kill_ms: f64) -> Outcome {
        let (ox, oy) = self.origin.iter().find(|o| o.0 == tg.id).map(|o| (o.1, o.2)).unwrap_or(self.cursor);
        let (vx, vy) = (tg.x - ox, tg.y - oy);
        let len = (vx * vx + vy * vy).sqrt().max(1e-6);
        let (ux, uy) = (vx / len, vy / len);
        let path: Vec<&Sample> = self.path.iter().filter(|s| s.t >= tg.spawn_ms && s.t <= kill_ms).collect();
        let reaction = path
            .iter()
            .find(|s| ((s.x - ox).powi(2) + (s.y - oy).powi(2)).sqrt() > 4.0)
            .map(|s| s.t - tg.spawn_ms)
            .unwrap_or(kill_ms - tg.spawn_ms);
        let furthest = path.iter().map(|s| (s.x - ox) * ux + (s.y - oy) * uy).fold(0.0, f64::max);
        let overshoot = ((furthest - len).max(0.0) / len * 100.0).min(100.0);
        let mut angle = vy.atan2(vx).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }
        Outcome { target_id: tg.id, reaction_ms: reaction, ttk_ms: kill_ms - tg.spawn_ms, flick_px: len, overshoot_pct: overshoot, angle_deg: angle }
    }

    pub fn live(&self) -> LiveStats {
        let hits = self.outcomes.len() as u32;
        let n = hits.max(1) as f64;
        let accuracy = if self.shots == 0 { 0.0 } else { hits as f64 / self.shots as f64 };
        let avg = |f: fn(&Outcome) -> f64| self.outcomes.iter().map(f).sum::<f64>() / n;
        let avg_ttk = avg(|o| o.ttk_ms);
        // 100 per kill, scaled by accuracy, bonus for kills under 500 ms
        let fast = self.outcomes.iter().filter(|o| o.ttk_ms < 500.0).count() as f64;
        let score = (hits as f64 * 100.0 * (0.5 + accuracy / 2.0) + fast * 25.0).round() as u32;
        LiveStats {
            shots: self.shots,
            hits,
            accuracy,
            avg_reaction_ms: avg(|o| o.reaction_ms),
            avg_ttk_ms: avg_ttk,
            overshoot_pct: avg(|o| o.overshoot_pct),
            score,
        }
    }

    pub fn report(&self) -> AimReport {
        let live = self.live();
        let (start, bin, bins) = (150.0, 25.0, 16usize);
        let mut hist = vec![0u32; bins];
        for o in &self.outcomes {
            let i = (((o.reaction_ms - start) / bin).floor().max(0.0) as usize).min(bins - 1);
            hist[i] += 1;
        }
        let mut sum = [0.0f64; 8];
        let mut cnt = [0u32; 8];
        for o in &self.outcomes {
            let s = (((o.angle_deg + 22.5) % 360.0) / 45.0) as usize % 8;
            sum[s] += o.overshoot_pct;
            cnt[s] += 1;
        }
        let mut by_dir = [0.0; 8];
        for i in 0..8 {
            by_dir[i] = if cnt[i] > 0 { sum[i] / cnt[i] as f64 } else { 0.0 };
        }
        let minutes = (self.cfg.duration_ms / 60_000.0).max(1e-6);
        AimReport {
            config: self.cfg.clone(),
            live,
            kills_per_min: live.hits as f64 / minutes,
            best_reaction_ms: self.outcomes.iter().map(|o| o.reaction_ms).fold(f64::INFINITY, f64::min).min(9999.0),
            reaction_hist: hist,
            hist_bin_ms: bin,
            hist_start_ms: start,
            overshoot_by_dir: by_dir,
            heatmap: self.heatmap.clone(),
            mapped_shots: self.heatmap.total(),
            outcomes: self.outcomes.clone(),
        }
    }
}

/// Play a full session with a simulated player. Drives the real engine, so it
/// doubles as sample data for the UI and as an end-to-end test.
pub fn simulate(cfg: AimConfig, skill: f64) -> AimReport {
    let mut eng = AimEngine::new(cfg.clone());
    let mut rng = Rng::new(cfg.seed.wrapping_mul(31).wrapping_add(7));
    let skill = skill.clamp(0.1, 1.0);
    eng.start();
    let mut t = 0.0;
    let mut cursor = (cfg.arena_w / 2.0, cfg.arena_h / 2.0);
    while t < cfg.duration_ms {
        let Some(tg) = eng.active().first().copied() else { break };
        let react = rng.normal(330.0 - 90.0 * skill, 35.0).max(140.0);
        let travel = rng.normal(260.0 - 80.0 * skill, 40.0).max(90.0);
        let over = (rng.normal(0.12 - 0.08 * skill, 0.05)).max(0.0);
        let (sx, sy) = cursor;
        let (dx, dy) = (tg.x - sx, tg.y - sy);
        let mut samples = vec![Sample { t: tg.spawn_ms.max(t), x: sx, y: sy }];
        let t0 = tg.spawn_ms.max(t) + react;
        for k in 1..=12 {
            let f = k as f64 / 12.0;
            // ease-out to (1 + over), then settle back to the target
            let e = 1.0 - (1.0 - f).powi(3);
            let p = if f < 0.75 { e * (1.0 + over) } else { 1.0 + over * (1.0 - f) * 4.0 };
            samples.push(Sample { t: t0 + travel * f, x: sx + dx * p, y: sy + dy * p });
        }
        eng.track(&samples);
        let mut shot_t = t0 + travel + rng.range(10.0, 60.0);
        // misses: a hasty shot that lands just outside the target
        if rng.next_f64() > 0.78 + 0.2 * skill {
            let a = rng.range(0.0, std::f64::consts::TAU);
            let d = tg.r * rng.range(1.15, 2.4);
            eng.shoot(shot_t, tg.x + a.cos() * d, tg.y + a.sin() * d);
            shot_t += rng.range(60.0, 140.0);
        }
        let jitter = tg.r * (0.75 - 0.35 * skill);
        let (hx, hy) = (tg.x + rng.normal(0.0, jitter * 0.5).clamp(-jitter, jitter), tg.y + rng.normal(0.0, jitter * 0.5).clamp(-jitter, jitter));
        eng.shoot(shot_t, hx, hy);
        cursor = (hx, hy);
        t = shot_t;
    }
    eng.report()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_uniformish() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let x = a.next_f64();
            assert_eq!(x, b.next_f64());
            assert!((0.0..1.0).contains(&x));
            sum += x;
        }
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn same_seed_same_schedule() {
        let mut e1 = AimEngine::new(AimConfig::default());
        let mut e2 = AimEngine::new(AimConfig::default());
        let a: Vec<_> = e1.start().iter().map(|t| (t.x, t.y)).collect();
        let b: Vec<_> = e2.start().iter().map(|t| (t.x, t.y)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn targets_stay_inside_arena() {
        let cfg = AimConfig { mode: AimMode::Grid, ..Default::default() };
        let mut e = AimEngine::new(cfg.clone());
        e.start();
        for i in 0..300 {
            let tg = e.active()[i % e.active().len()];
            assert!(tg.x >= MARGIN && tg.x <= cfg.arena_w - MARGIN);
            assert!(tg.y >= MARGIN && tg.y <= cfg.arena_h - MARGIN);
            let r = e.shoot(i as f64 * 100.0, tg.x, tg.y);
            assert_eq!(r.hit, Some(tg.id));
            assert_eq!(e.active().len(), 3);
        }
    }

    #[test]
    fn miss_and_hit_accounting() {
        let mut e = AimEngine::new(AimConfig::default());
        let tg = e.start()[0];
        assert!(e.shoot(100.0, tg.x + tg.r * 2.0, tg.y).hit.is_none());
        let r = e.shoot(300.0, tg.x, tg.y);
        assert_eq!(r.hit, Some(tg.id));
        assert_eq!(r.spawned.len(), 1);
        assert_eq!(r.live.shots, 2);
        assert!((r.live.accuracy - 0.5).abs() < 1e-12);
        assert_eq!(e.report().heatmap.total(), 2);
    }

    #[test]
    fn overshoot_is_measured_along_flick() {
        let cfg = AimConfig::default();
        let mut e = AimEngine::new(cfg.clone());
        let tg = e.start()[0];
        let (ox, oy) = (cfg.arena_w / 2.0, cfg.arena_h / 2.0);
        let (dx, dy) = (tg.x - ox, tg.y - oy);
        e.track(&[
            Sample { t: 0.0, x: ox, y: oy },
            Sample { t: 200.0, x: ox + dx * 0.05, y: oy + dy * 0.05 },
            Sample { t: 350.0, x: ox + dx * 1.10, y: oy + dy * 1.10 },
            Sample { t: 420.0, x: tg.x, y: tg.y },
        ]);
        e.shoot(450.0, tg.x, tg.y);
        let o = e.report().outcomes[0];
        assert!((o.overshoot_pct - 10.0).abs() < 1e-6, "{}", o.overshoot_pct);
        assert_eq!(o.reaction_ms, 200.0);
        assert_eq!(o.ttk_ms, 450.0);
    }

    #[test]
    fn simulated_session_is_plausible() {
        let r = simulate(AimConfig { duration_ms: 30_000.0, ..Default::default() }, 0.8);
        assert!(r.live.hits > 30, "hits {}", r.live.hits);
        assert!(r.live.accuracy > 0.75 && r.live.accuracy <= 1.0);
        assert!(r.live.avg_reaction_ms > 150.0 && r.live.avg_reaction_ms < 400.0);
        assert_eq!(r.reaction_hist.iter().sum::<u32>(), r.live.hits);
        assert!(r.heatmap.total() >= r.live.hits);
    }
}
