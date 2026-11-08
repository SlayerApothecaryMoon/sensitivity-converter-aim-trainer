'use strict';
// SensMatch UI. All math, scoring and persistence live in Rust (src-tauri/src).
// Without Tauri (plain browser) the MOCK table below mirrors the Rust commands.
const tauri = window.__TAURI__;
const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)];
const fx = (v, d = 2) => Number(v).toFixed(d);
const fk = v => Math.round(v).toLocaleString('en-US');
const sgn = (v, d = 2) => (v >= 0 ? '+' : '−') + Math.abs(v).toFixed(d);
const DEG = Math.PI / 180;

const GAMES = [
  ['valorant', 'Valorant', 'Valorant', 'VAL', 'UE4', 0.07, 'h169', 103, 103, 103, 3],
  ['cs2', 'Counter-Strike 2', 'Counter-Strike', 'CS2', 'Source 2', 0.022, 'h43', 90, 90, 90, 3],
  ['apex', 'Apex Legends', 'Apex', 'APX', 'Source', 0.022, 'h43', 90, 70, 110, 3],
  ['fortnite', 'Fortnite', 'Fortnite', 'FN', 'UE5', 0.005555, 'h169', 80, 80, 80, 1],
  ['overwatch2', 'Overwatch 2', 'Overwatch 2', 'OW2', 'OW Engine', 0.0066, 'h169', 103, 80, 103, 2],
  ['r6s', 'Rainbow Six Siege', 'Siege', 'R6S', 'AnvilNext', 0.005729578, 'v', 60, 60, 90, 0],
  ['cod', 'Call of Duty', 'CoD', 'COD', 'IW 9', 0.0066, 'h43', 80, 60, 120, 2],
  ['the_finals', 'The Finals', 'The Finals', 'FIN', 'UE5', 0.0066, 'v', 71, 60, 100, 2],
  ['deadlock', 'Deadlock', 'Deadlock', 'DLK', 'Source 2', 0.044, 'h43', 90, 90, 90, 3],
  ['marvel_rivals', 'Marvel Rivals', 'Rivals', 'MR', 'UE5', 0.0175, 'h169', 90, 90, 90, 2],
].map(([id, name, short, code, engine, yaw, kind, fov, fmin, fmax, dec]) => ({ id, name, short, code, engine, yaw, kind, fov, fmin, fmax, dec }));
const G = id => GAMES.find(g => g.id === id) || GAMES[0];

// ---------- local mirror of the Rust engine (browser preview only) ----------
const vfov = (g, f) => g.kind === 'v' ? f : 2 * Math.atan(Math.tan(f * DEG / 2) / (g.kind === 'h43' ? 4 / 3 : 16 / 9)) / DEG;
const hfov = (v, a = 16 / 9) => 2 * Math.atan(Math.tan(v * DEG / 2) * a) / DEG;
const cm360 = (yaw, s, dpi) => 360 / (yaw * s) / dpi * 2.54;
const sensFor = (g, cm, dpi) => Math.round(360 * 2.54 / (cm * dpi * g.yaw) * 10 ** g.dec) / 10 ** g.dec;
const mratio = (vf, vt, p) => { const a = Math.tan(vf * DEG / 2), b = Math.tan(vt * DEG / 2); return p <= 1e-6 ? b / a : Math.atan(p / 100 * b) / Math.atan(p / 100 * a); };
const ll = (a, b, t) => Math.exp(Math.log(a) + (Math.log(b) - Math.log(a)) * t);
const lrng = seed => { let s = seed >>> 0 || 1; return () => (s = (s * 1664525 + 1013904223) >>> 0) / 4294967296; };

function convertLocal({ req: r }) {
  const f = G(r.from_game), t = G(r.to_game), dpiT = r.to_dpi || r.dpi;
  const vf = vfov(f, r.from_fov ?? f.fov), vt = vfov(t, r.to_fov ?? t.fov);
  const ratio = r.method.kind === 'monitor' ? mratio(vf, vt, r.method.percent) : 1;
  const ex = f.yaw * r.sens * r.dpi * ratio / (t.yaw * dpiT), s = Math.round(ex * 10 ** t.dec) / 10 ** t.dec;
  return { sens_exact: ex, sens: s, rounding_error_pct: (s / ex - 1) * 100, cm360_from: cm360(f.yaw, r.sens, r.dpi),
    cm360_to: cm360(t.yaw, s, dpiT), in360_to: cm360(t.yaw, s, dpiT) / 2.54, counts360_to: 360 / (t.yaw * s),
    edpi_from: r.sens * r.dpi, edpi_to: s * dpiT, hfov_from: hfov(vf), hfov_to: hfov(vt), vfov_from: vf, vfov_to: vt, fov_ratio: ratio, notes: [] };
}

const CFG = {
  cs2: ['cfg\\autoexec.cfg', s => [`sensitivity "${s}"`, 'm_yaw "0.022"']],
  deadlock: ['cfg\\autoexec.cfg', s => [`sensitivity "${s}"`]],
  apex: ['local\\settings.cfg', s => [`mouse_sensitivity "${s}"`]],
  r6s: ['GameSettings.ini', s => [`MouseYawSensitivity=${s}`, `MousePitchSensitivity=${s}`]],
  fortnite: ['Settings > Mouse & Keyboard', s => [`Mouse X Sensitivity: ${s}%`, `Mouse Y Sensitivity: ${s}%`]],
  valorant: ['Settings > General > Mouse', s => [`Sensitivity: Aim ${s}`]],
};

const H = 3600;
const PROFILES = [
  ['Main - tactical', 'valorant', 0.312, 1600, 'feel', 1], ['CS2 from Valorant', 'cs2', 0.993, 1600, 'convert', 7],
  ['Apex hipfire', 'apex', 1.6, 800, 'convert', 25], ['Overwatch tracking', 'overwatch2', 4.73, 800, 'convert', 72],
  ['CS2 autoexec', 'cs2', 1.25, 800, 'import', 166], ['Fortnite build', 'fortnite', 6.4, 800, 'convert', 390],
].map(([name, game, sens, dpi, source, age], i) => ({ id: 6 - i, name, game, sens, dpi, source, fov: null, note: '', created: Date.now() / 1000 - age * H, cm360: cm360(G(game).yaw, sens, dpi) }));

const LA = {}, LF = {};
function lspawn(t) {
  const c = LA.cfg, r = c.radius, m = 24 + r;
  let x, y, k = 0;
  do { x = m + LA.r() * (c.arena_w - 2 * m); y = m + LA.r() * (c.arena_h - 2 * m); }
  while (k++ < 30 && Math.hypot(x - LA.last[0], y - LA.last[1]) < (c.mode === 'microflick' ? 0 : 220));
  if (c.mode === 'microflick') { x = Math.min(c.arena_w - m, Math.max(m, LA.last[0] + (LA.r() - .5) * 260)); y = Math.min(c.arena_h - m, Math.max(m, LA.last[1] + (LA.r() - .5) * 180)); }
  LA.last = [x, y];
  const tg = { id: LA.id++, x, y, r, spawn_ms: t };
  LA.act.push(tg);
  return tg;
}
function llive() {
  const o = LA.out, n = Math.max(1, o.length), acc = LA.shots ? o.length / LA.shots : 0, avg = k => o.reduce((s, x) => s + x[k], 0) / n;
  return { shots: LA.shots, hits: o.length, accuracy: acc, avg_reaction_ms: avg('re'), avg_ttk_ms: avg('ttk'), overshoot_pct: avg('os'), score: Math.round(o.length * 100 * (0.5 + acc / 2) + o.filter(x => x.ttk < 500).length * 25) };
}
function feelOpen(s) { s.rounds.push({ index: s.rounds.length + 1, lo: s.lo, hi: s.hi, a_cm360: ll(s.lo, s.hi, .25), b_cm360: ll(s.lo, s.hi, .75), choice: null, score_a: null, score_b: null }); }

function simReport() {
  const r = lrng(0x5e45), g = (m, s) => m + s * Math.sqrt(-2 * Math.log(r() || 1e-9)) * Math.cos(6.2832 * r());
  const N = 17, cells = Array(N * N).fill(0), hist = Array(16).fill(0), dS = Array(8).fill(0), dN = Array(8).fill(0);
  const add = (dx, dy) => { const cx = Math.floor((dx / 3 + 1) / 2 * N), cy = Math.floor((dy / 3 + 1) / 2 * N); if (cx >= 0 && cy >= 0 && cx < N && cy < N) cells[cy * N + cx]++; };
  let hits = 0, shots = 0, rs = 0, ts = 0, os = 0, best = 999, fast = 0;
  for (let i = 0; i < 131; i++) {
    const re = Math.max(160, g(266, 36)), ttk = re + Math.max(110, g(186, 38)), a = r() * 8 | 0;
    const o = Math.max(0, g(5.2, 2.6)) * (a % 4 === 0 ? 1.45 : a % 2 ? 1.05 : 0.8);
    dS[a] += o; dN[a]++; rs += re; ts += ttk; os += o; best = Math.min(best, re); fast += ttk < 500;
    hist[Math.min(15, Math.max(0, (re - 150) / 25 | 0))]++; hits++; shots++;
    add(g(0.12, 0.36), g(0, 0.3));
    if (r() > 0.9) { shots++; const s = r() > .5 ? 1 : -1; add(s * (1.2 + r() * 1.4), g(0, 0.6)); }
  }
  const acc = hits / shots;
  return { config: { mode: 'flick', duration_ms: 60000, radius: 18 }, live: { shots, hits, accuracy: acc, avg_reaction_ms: rs / hits, avg_ttk_ms: ts / hits, overshoot_pct: os / hits, score: Math.round(hits * 100 * (0.5 + acc / 2) + fast * 25) },
    kills_per_min: hits, best_reaction_ms: best, reaction_hist: hist, hist_bin_ms: 25, hist_start_ms: 150, overshoot_by_dir: dS.map((s, i) => s / Math.max(1, dN[i])), heatmap: { size: N, extent: 3, cells }, mapped_shots: shots };
}

const MOCK = {
  convert: convertLocal,
  export_config: ({ game, sens }) => { const [target, f] = CFG[game] || ['In-game settings', s => [`Mouse sensitivity ${s}`]]; return { target, lines: f(sens.toFixed(G(game).dec)) }; },
  profiles_list: () => PROFILES,
  profile_save: ({ profile: p }) => { const n = { ...p, id: PROFILES.length + 1, created: Date.now() / 1000, cm360: cm360(G(p.game).yaw, p.sens, p.dpi) }; PROFILES.unshift(n); return n; },
  system_status: () => ({ mouse: { enhance_pointer_precision: false, pointer_speed: 10, screen_w: 2560, screen_h: 1440, aspect: 16 / 9, warnings: [] }, running_game: 'cs2', version: '1.0.0' }),
  import_cs2: () => 1.25,
  verify_plan: ({ game, sens, dpi }) => { const c = 360 / (G(game).yaw * sens); return { counts: c, cm: c / dpi * 2.54 }; },
  aim_start: ({ config }) => { Object.assign(LA, { cfg: config, r: lrng(config.seed || Date.now()), out: [], shots: 0, act: [], id: 1, last: [config.arena_w / 2, config.arena_h / 2] }); return Array.from({ length: config.mode === 'grid' ? 3 : 1 }, () => lspawn(0)); },
  aim_track: () => null,
  aim_shoot: ({ t, x, y }) => {
    LA.shots++;
    const i = LA.act.findIndex(g => Math.hypot(g.x - x, g.y - y) <= g.r), spawned = [];
    let hit = null;
    if (i >= 0) { const g = LA.act.splice(i, 1)[0], ttk = t - g.spawn_ms; hit = g.id; LA.out.push({ ttk, re: ttk * 0.58, os: Math.abs(x - g.x) / g.r * 4 }); spawned.push(lspawn(t)); }
    return { hit, spawned, live: llive() };
  },
  aim_finish: () => { const rep = simReport(); if (LA.out && LA.out.length > 4) rep.live = llive(); return rep; },
  aim_report: simReport,
  sessions_list: () => [[0.6, 'flick', 'cs2', 1.273, .910, 267, 455, 14974], [0.95, 'flick', 'cs2', 1.3, .896, 281, 473, 14210], [24.4, 'grid', 'valorant', 0.4, .931, 302, 431, 15880], [72.1, 'microflick', 'valorant', 0.42, .872, 274, 402, 13140]]
    .map(([h, mode, game, sens, accuracy, avg_reaction_ms, avg_ttk_ms, score], i) => ({ id: 9 - i, finished: Date.now() / 1000 - h * H, mode, game, sens, cm360: cm360(G(game).yaw, sens, 800), accuracy, avg_reaction_ms, avg_ttk_ms, overshoot_pct: 5.9, score })),
  history_trend: () => ({ sessions_7d: 11, avg_accuracy_7d: .886, accuracy_delta_pts: 2.4, ttk_delta_ms: -18, best_score: 15880, best_cm360: 41 }),
  feel_start: ({ config: c }) => { const s = { config: c, lo: c.base_cm360 * (1 - c.spread), hi: c.base_cm360 * (1 + c.spread), rounds: [], done: false }; feelOpen(s); LF.s = s; return structuredClone(s); },
  feel_submit: ({ choice }) => {
    const s = LF.s, r = s.rounds.at(-1), m = ll(s.lo, s.hi, .5);
    r.choice = choice;
    if (choice === 'a') s.hi = m; else if (choice === 'b') s.lo = m; else { s.lo = r.a_cm360; s.hi = r.b_cm360; }
    if ((s.hi / s.lo - 1) * 100 <= s.config.tolerance_pct || s.rounds.length >= s.config.max_rounds) s.done = true; else feelOpen(s);
    return structuredClone(s);
  },
  feel_undo: () => { const s = LF.s; if (s.rounds.length > 1 && !s.done) s.rounds.pop(); const r = s.rounds.at(-1); if (r.choice) { r.choice = null; s.lo = r.lo; s.hi = r.hi; s.done = false; } return structuredClone(s); },
  feel_scores: ({ a, b }) => { const c = x => x.accuracy * 1000 / x.avg_ttk_ms * (1 - x.overshoot_pct / 100); if (!a || !b) return null; const d = (c(a) - c(b)) / Math.max(c(a), c(b)); return Math.abs(d) < .03 ? 'same' : d > 0 ? 'a' : 'b'; },
};
const api = (cmd, args = {}) => tauri ? tauri.core.invoke(cmd, args) : Promise.resolve().then(() => MOCK[cmd](args));

function toast(msg) { const t = $('#toast'); t.textContent = msg; t.classList.add('on'); clearTimeout(toast.h); toast.h = setTimeout(() => t.classList.remove('on'), 2400); }

// ---------- converter ----------
const S = { from: { game: 'valorant', sens: '0.400', dpi: 800, fov: null }, to: { game: 'cs2', dpi: 800, fov: null }, mi: 0, res: null };
const METHODS = [
  [{ kind: 'distance360' }, '360° distance', 'Same mouse travel for a full turn. Keeps muscle memory for tracking and turning.'],
  [{ kind: 'monitor', percent: 0 }, 'MDV 0%', 'Matches speed right around the crosshair (zoom ratio). Best for micro-corrections.'],
  [{ kind: 'monitor', percent: 56.25 }, '56.25%', 'Matches flicks to a point 56.25% of the way to the top edge. Balanced default.'],
  [{ kind: 'monitor', percent: 75 }, '75%', 'Matches medium flicks at 75% of the vertical half-height.'],
  [{ kind: 'monitor', percent: 100 }, '100%', 'Matches flicks to the top edge of the screen. Favors wide flicks.'],
];

function panel(side) {
  const s = S[side], g = G(s.game), from = side === 'from', fov = s.fov ?? g.fov;
  $('#p-' + side).innerHTML = `<div class="ph"><span class="lbl">${from ? 'From' : 'To'}</span><span class="mono">yaw ${g.yaw}° · ${g.engine}</span></div>
    <div class="tiles">${GAMES.map(x => `<button class="tile${x.id === g.id ? ' on' : ''}" data-side="${side}" data-g="${x.id}"><b>${x.code}</b><span>${x.short}</span></button>`).join('')}</div>
    <div class="fields">
      <label class="f big"><span class="lbl">${from ? 'Sensitivity' : 'Converted'}</span>${from ? `<input id="in-sens" inputmode="decimal" value="${s.sens}">` : '<output id="out-sens"></output>'}</label>
      <label class="f"><span class="lbl">DPI</span><input data-k="dpi" data-side="${side}" inputmode="numeric" value="${s.dpi}"></label>
      <label class="f"><span class="lbl">FOV</span>${g.fmin !== g.fmax ? `<input data-k="fov" data-side="${side}" value="${fov}">` : `<span class="ro">${fov}</span>`}</label>
    </div><div class="meta" id="meta-${side}"></div>`;
}

function renderConvert() {
  panel('from'); panel('to');
  $('#seg-method').innerHTML = METHODS.map((m, i) => `<button data-mi="${i}" class="${i === S.mi ? 'on' : ''}">${m[1]}</button>`).join('');
  $('#method-desc').textContent = METHODS[S.mi][2];
  return update();
}

function request() {
  return { from_game: S.from.game, to_game: S.to.game, sens: +S.from.sens, dpi: +S.from.dpi, to_dpi: +S.to.dpi || null, from_fov: S.from.fov, to_fov: S.to.fov, method: METHODS[S.mi][0], aspect: 16 / 9 };
}

async function update() {
  const req = request();
  if (!(req.sens > 0 && req.dpi > 0)) return;
  try {
    S.res = await api('convert', { req });
    showResult(S.res);
    const e = await api('export_config', { game: S.to.game, sens: S.res.sens, fov: S.to.fov });
    $('#cfg-line').innerHTML = e.lines.join('&nbsp;&nbsp;') + `<i>${e.target}</i>`;
  } catch (err) { toast(String(err)); }
}

function ruler(r) {
  const W = ($('#result').clientWidth || 1000) - 40, max = Math.max(60, Math.ceil((Math.max(r.cm360_to, r.cm360_from) + 8) / 10) * 10), x = v => 20 + Math.min(v, max) / max * (W - 40);
  let s = '';
  for (let c = 0; c <= max; c++) {
    const big = c % 10 === 0;
    s += `<path d="M${x(c)} 36v${big ? -12 : c % 5 ? -4 : -8}" stroke="rgba(255,255,255,${big ? .5 : .18})"/>`;
    if (big) s += `<text x="${x(c)}" y="50" text-anchor="middle">${c} cm</text>`;
  }
  const a = x(r.cm360_from), b = x(r.cm360_to), right = b > W - 120;
  return `<svg viewBox="0 0 ${W} 54"><text x="20" y="11">MOUSEPAD TRAVEL FOR ONE 360°</text><path d="M20 36H${W - 20}" stroke="rgba(255,255,255,.22)"/>
    <rect x="20" y="34" width="${Math.max(0, b - 20)}" height="3" fill="#ff3b30"/>${s}
    ${Math.abs(a - b) > 2 ? `<path d="M${a} 16v20" stroke="#fff" stroke-dasharray="2 2"/>` : ''}<path d="M${b} 14v22" stroke="#ff3b30" stroke-width="2"/>
    <text class="v" x="${right ? b - 6 : b + 6}" y="23" text-anchor="${right ? 'end' : 'start'}">${fx(r.cm360_to)} cm</text></svg>`;
}

function showResult(r) {
  const f = G(S.from.game), t = G(S.to.game);
  $('#out-sens').textContent = r.sens.toFixed(t.dec);
  $('#meta-from').innerHTML = `<span>eDPI <b>${fk(r.edpi_from)}</b></span><span>hFOV <b>${fx(r.hfov_from, 1)}°</b></span><span><b>${fx(r.cm360_from)}</b> cm/360</span>`;
  $('#meta-to').innerHTML = `<span>eDPI <b>${fk(r.edpi_to)}</b></span><span>hFOV <b>${fx(r.hfov_to, 1)}°</b></span><span>exact <b>${r.sens_exact.toFixed(5)}</b></span>`;
  $('#result').innerHTML = `
    <div><span class="lbl">Physical distance · ${t.short}</span><div class="huge"><span>${fx(r.cm360_to)}</span><small>cm / 360°</small></div>
      <div class="sm mono">${fx(r.in360_to)} in · ${fk(r.counts360_to)} counts · ${(360 / r.counts360_to).toFixed(4)}°/count</div></div>
    <div><span class="lbl">eDPI</span><div class="num">${fk(r.edpi_from)}<i> → </i>${fk(r.edpi_to)}</div><div class="sm">${f.short} → ${t.short} units</div></div>
    <div><span class="lbl">Horizontal FOV</span><div class="num">${fx(r.hfov_from, 1)}°<i> → </i>${fx(r.hfov_to, 1)}°</div><div class="sm">vertical ${fx(r.vfov_from, 1)}° → ${fx(r.vfov_to, 1)}°</div></div>
    <div><span class="lbl">Rounding</span><div class="num">${sgn(r.rounding_error_pct)}%</div><div class="sm">${t.short} stores ${t.dec} decimals</div></div>
    <div class="ruler">${ruler(r)}</div>
    <div class="cfg"><span class="lbl">Config</span><code id="cfg-line"></code><button class="btn" id="b-copy"><svg class="ic"><use href="#i-copy"/></svg>Copy</button></div>`;
}

// ---------- sidebar ----------
const ago = ts => { const h = (Date.now() / 1000 - ts) / H; return h < 1 ? 'just now' : h < 24 ? `${h | 0} h ago` : `${h / 24 | 0} d ago`; };
async function renderSide() {
  const list = await api('profiles_list');
  $('#profiles').innerHTML = list.map((p, i) => `<div class="prof${i === 0 ? ' on' : ''}" data-p="${p.id}"><span class="code">${G(p.game).code}</span>
    <div><b>${p.name}</b><span>${p.sens} @ ${p.dpi} · ${ago(p.created)}</span></div><em>${fx(p.cm360, 1)}</em></div>`).join('');
  S.profiles = list;
  const st = await api('system_status'), m = st.mouse;
  $('#device').innerHTML = `<div>Display <b>${m.screen_w} × ${m.screen_h}</b></div><div>Pointer speed <b>${m.pointer_speed}/20</b></div>
    <div>Enhance precision <b>${m.enhance_pointer_precision ? 'On' : 'Off'}</b></div><div>Mouse <b>${S.from.dpi} DPI</b></div>`;
  setDetect(st.running_game);
}
const setDetect = id => { $('#detect').innerHTML = id ? `<i></i>${G(id).name} running` : '<span class="t3">No supported game running</span>'; };

// ---------- aim test ----------
const ZERO = { shots: 0, hits: 0, accuracy: 0, avg_reaction_ms: 0, avg_ttk_ms: 0, overshoot_pct: 0, score: 0 };
const A = { mode: 'flick', dur: 60000, t: 0, running: false, targets: [], cursor: { x: 520, y: 330 }, trail: [], pops: [], buf: [], live: { ...ZERO }, w: 0, h: 0 };
const MODES = [['flick', 'Flick'], ['microflick', 'Microflick'], ['grid', 'Grid']];

function sizeArena() {
  const cv = $('#cv'), dpr = devicePixelRatio || 1;
  A.w = cv.clientWidth; A.h = cv.clientHeight;
  cv.width = A.w * dpr; cv.height = A.h * dpr;
}

function drawArena() {
  const cv = $('#cv'), c = cv.getContext('2d'), dpr = devicePixelRatio || 1, w = A.w, h = A.h;
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  c.fillStyle = '#050505'; c.fillRect(0, 0, w, h);
  c.beginPath();
  for (let x = (w / 2) % 48; x < w; x += 48) { c.moveTo((x | 0) + .5, 0); c.lineTo((x | 0) + .5, h); }
  for (let y = (h / 2) % 48; y < h; y += 48) { c.moveTo(0, (y | 0) + .5); c.lineTo(w, (y | 0) + .5); }
  c.strokeStyle = 'rgba(255,255,255,.035)'; c.lineWidth = 1; c.stroke();
  c.beginPath(); c.moveTo(w / 2 - 6, h / 2); c.lineTo(w / 2 + 6, h / 2); c.moveTo(w / 2, h / 2 - 6); c.lineTo(w / 2, h / 2 + 6);
  c.strokeStyle = 'rgba(255,255,255,.12)'; c.stroke();
  if (A.trail.length > 1) {
    c.beginPath(); A.trail.forEach((p, i) => i ? c.lineTo(p.x, p.y) : c.moveTo(p.x, p.y));
    c.strokeStyle = 'rgba(255,255,255,.2)'; c.lineWidth = 1.25; c.stroke();
  }
  for (const m of A.marks || []) { c.fillStyle = 'rgba(255,255,255,.35)'; c.fillRect(m.x - 1.5, m.y - 1.5, 3, 3); }
  for (const p of A.pops) {
    const k = p.age / 320; if (k >= 1) continue;
    c.beginPath(); c.arc(p.x, p.y, p.r * (1 + k * 1.3), 0, 7); c.strokeStyle = `rgba(255,59,48,${1 - k})`; c.lineWidth = 2; c.stroke();
  }
  for (const t of A.targets) {
    const r = t.r * (t.scale ?? 1); if (r <= 0) continue;
    c.beginPath(); c.arc(t.x, t.y, r + 5, 0, 7); c.strokeStyle = 'rgba(255,59,48,.22)'; c.lineWidth = 1; c.stroke();
    c.beginPath(); c.arc(t.x, t.y, r, 0, 7); c.fillStyle = '#ff3b30'; c.fill();
    c.beginPath(); c.arc(t.x, t.y, r * .38, 0, 7); c.fillStyle = 'rgba(0,0,0,.3)'; c.fill();
  }
  const { x, y } = A.cursor;
  c.beginPath();
  for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) { c.moveTo(x + dx * 4, y + dy * 4); c.lineTo(x + dx * 12, y + dy * 12); }
  c.strokeStyle = 'rgba(0,0,0,.8)'; c.lineWidth = 4; c.stroke(); c.strokeStyle = '#ff3b30'; c.lineWidth = 2; c.stroke();
  c.fillStyle = '#ff3b30'; c.fillRect(x - 1, y - 1, 2, 2);
}

function renderHud() {
  $('#seg-mode').innerHTML = MODES.map(([k, l]) => `<button data-mode="${k}" class="${k === A.mode ? 'on' : ''}">${l}</button>`).join('');
  const r = S.res, t = G(S.to.game);
  $('#a-sens').textContent = r ? `${t.short} ${r.sens.toFixed(t.dec)} · ${fx(r.cm360_to)} cm/360` : '';
  $('#b-aim').innerHTML = A.running ? '<svg class="ic"><use href="#i-stop"/></svg>End session' : '<svg class="ic"><use href="#i-play"/></svg>Start';
  renderTimer(); renderStats();
}
function renderTimer() { const left = Math.max(0, A.dur - A.t) / 1000; $('#a-timer').textContent = `${String(left / 60 | 0).padStart(2, '0')}:${(left % 60).toFixed(1).padStart(4, '0')}`; }
function renderStats() {
  const L = A.live;
  $('#a-stats').innerHTML = [['Accuracy', fx(L.accuracy * 100, 1), '%', 1], ['Avg reaction', Math.round(L.avg_reaction_ms), 'ms'], ['Time to kill', Math.round(L.avg_ttk_ms), 'ms'],
    ['Overshoot', fx(L.overshoot_pct, 1), '%'], ['Hits', L.hits, `/ ${L.shots}`], ['Score', fk(L.score), '']]
    .map(([l, v, u, hot]) => `<div class="stat${hot ? ' hot' : ''}"><span class="lbl">${l}</span><b>${v}<small>${u}</small></b></div>`).join('');
}

// Pixels per raw count so the arena honours the converted sensitivity:
// degrees per count (from cm/360) times pixels per degree of the target FOV.
const pxPerCount = () => S.res ? (360 / S.res.counts360_to) * (A.w / S.res.hfov_to) : 1;

async function aimStart() {
  sizeArena();
  Object.assign(A, { running: true, t: 0, t0: performance.now(), trail: [], pops: [], marks: [], buf: [], live: { ...ZERO }, cursor: { x: A.w / 2, y: A.h / 2 } });
  A.targets = await api('aim_start', { config: { mode: A.mode, seed: 0, duration_ms: A.dur, arena_w: A.w, arena_h: A.h, radius: 18 } });
  $('#arena').requestPointerLock?.();
  renderHud(); requestAnimationFrame(aimTick);
}
function aimTick(now) {
  if (!A.running) return;
  A.t = now - A.t0;
  if (A.t >= A.dur) return aimEnd();
  if (A.buf.length) { api('aim_track', { samples: A.buf }); A.buf = []; }
  A.pops.forEach(p => p.age += 16); A.pops = A.pops.filter(p => p.age < 320);
  A.trail = A.trail.filter(p => A.t - p.t < 220);
  drawArena(); renderTimer();
  requestAnimationFrame(aimTick);
}
async function aimEnd() {
  A.running = false; document.exitPointerLock?.();
  R = await api('aim_finish', { meta: S.res ? { game: S.to.game, sens: S.res.sens, cm360: S.res.cm360_to } : null }).catch(() => null);
  renderHud(); location.hash = '#results';
}
function aimMove(e) {
  const b = $('#arena').getBoundingClientRect();
  if (document.pointerLockElement) {
    const k = pxPerCount();
    A.cursor = { x: Math.min(A.w, Math.max(0, A.cursor.x + e.movementX * k)), y: Math.min(A.h, Math.max(0, A.cursor.y + e.movementY * k)) };
  } else A.cursor = { x: e.clientX - b.left, y: e.clientY - b.top };
  if (A.running) { const s = { t: A.t, ...A.cursor }; A.trail.push(s); A.buf.push(s); } else drawArena();
}
async function aimShoot() {
  if (!A.running) return;
  const { x, y } = A.cursor, res = await api('aim_shoot', { t: A.t, x, y });
  if (res.hit) { const tg = A.targets.find(t => t.id === res.hit); if (tg) A.pops.push({ ...tg, age: 0 }); A.targets = A.targets.filter(t => t.id !== res.hit).concat(res.spawned); }
  else A.marks.push({ x, y });
  A.live = res.live; renderStats();
}

// ---------- results ----------
let R = null;
const when = ts => { const d = new Date(ts * 1000), n = new Date(), hm = d.toTimeString().slice(0, 5), days = Math.round((new Date(n.toDateString()) - new Date(d.toDateString())) / 864e5);
  return (days === 0 ? 'Today' : days === 1 ? 'Yesterday' : d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })) + ' ' + hm; };
async function renderResults() {
  if (!R) R = await api('aim_report');
  const L = R.live, t = G(S.to.game), r = S.res, list = await api('sessions_list', { limit: 4 }), tr = await api('history_trend'), last = list[0];
  $('#r-sub').textContent = `${R.config.mode[0].toUpperCase() + R.config.mode.slice(1)} · ${R.config.duration_ms / 1000} s · ${r ? `${t.short} ${r.sens.toFixed(t.dec)} @ ${S.to.dpi || S.from.dpi} DPI · ${fx(r.cm360_to)} cm/360` : ''}${last ? ' · ' + when(last.finished) : ''}`;
  $('#kpis').innerHTML = [['Accuracy', fx(L.accuracy * 100, 1), '%', `${sgn(tr.accuracy_delta_pts, 1)} vs 7-day avg`, 1], ['Avg reaction', Math.round(L.avg_reaction_ms), 'ms', `best ${Math.round(R.best_reaction_ms)} ms`],
    ['Time to kill', Math.round(L.avg_ttk_ms), 'ms', `${sgn(tr.ttk_delta_ms, 0)} ms vs last`], ['Overshoot', fx(L.overshoot_pct, 1), '%', 'of flick distance'], ['Kills / min', Math.round(R.kills_per_min), '', tr.best_cm360 ? `aims best at ${fx(tr.best_cm360, 0)} cm/360` : `score ${fk(L.score)}`]]
    .map(([l, v, u, s, up]) => `<div class="kpi"><span class="lbl">${l}</span><b>${v}<small>${u}</small></b><span class="${up ? 'up' : ''}">${s}</span></div>`).join('');
  heat(R.heatmap); hist(R); polar(R.overshoot_by_dir);
  const rows = list.map((x, i) => `<tr class="${i ? '' : 'on'}"><td>${when(x.finished)}</td><td>${x.mode[0].toUpperCase() + x.mode.slice(1)}</td><td class="mono">${G(x.game).short} ${x.sens.toFixed(G(x.game).dec)}</td><td class="mono">${fx(x.cm360)}</td><td class="mono r">${fx(x.accuracy * 100, 1)}%</td><td class="mono r">${Math.round(x.avg_reaction_ms)} ms</td><td class="mono r">${fk(x.score)}</td></tr>`);
  $('#sessions').innerHTML = '<tr><th>Date</th><th>Mode</th><th>Sensitivity</th><th>cm/360</th><th class="r">Accuracy</th><th class="r">Reaction</th><th class="r">Score</th></tr>' + rows.join('');
}

function heat(hm) {
  const cv = $('#heat'), dpr = devicePixelRatio || 1, S2 = 236, n = hm.size, cs = S2 / n, max = Math.max(...hm.cells, 1);
  cv.width = S2 * dpr; cv.height = S2 * dpr;
  const c = cv.getContext('2d'); c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, S2, S2);
  const ramp = v => { const k = Math.pow(v, .7); return k < .5 ? `rgb(${20 + k * 2 * 72},${20 + k * 2 * 3},${20 + k * 2 * 1})` : `rgb(${92 + (k - .5) * 2 * 163},${23 + (k - .5) * 2 * 36},${18 + (k - .5) * 2 * 30})`; };
  hm.cells.forEach((v, i) => { c.fillStyle = v ? ramp(v / max) : '#0f0f0f'; c.fillRect((i % n) * cs + .5, (i / n | 0) * cs + .5, cs - 1, cs - 1); });
  c.strokeStyle = 'rgba(255,255,255,.75)'; c.lineWidth = 1; c.setLineDash([3, 3]);
  c.beginPath(); c.arc(S2 / 2, S2 / 2, S2 / 2 / hm.extent, 0, 7); c.stroke(); c.setLineDash([]);
  c.strokeStyle = 'rgba(255,255,255,.25)'; c.beginPath(); c.moveTo(S2 / 2 + .5, 0); c.lineTo(S2 / 2 + .5, S2); c.moveTo(0, S2 / 2 + .5); c.lineTo(S2, S2 / 2 + .5); c.stroke();
  $('#heat-n').textContent = `${R.mapped_shots} shots · ±${hm.extent} r`; $('#heat-max').textContent = max;
}

function hist(rep) {
  const h = rep.reaction_hist, max = Math.max(...h, 1), W = 420, x0 = 30, y0 = 206, bw = (W - x0 - 6) / h.length, top = Math.ceil(max / 5) * 5;
  let s = '';
  for (let v = 0; v <= top; v += 5) { const y = y0 - v / top * 180; s += `<path d="M${x0} ${y + .5}H${W - 4}" stroke="rgba(255,255,255,${v ? .06 : .24})"/><text x="${x0 - 6}" y="${y + 3}" text-anchor="end">${v}</text>`; }
  h.forEach((v, i) => { const bh = v / top * 180; s += `<rect x="${x0 + i * bw + 1.5}" y="${y0 - bh}" width="${bw - 3}" height="${bh}" fill="rgba(255,255,255,${v === max ? .9 : .32})"/>`; if (i % 2 === 0) s += `<text x="${x0 + i * bw}" y="${y0 + 16}" text-anchor="middle">${rep.hist_start_ms + i * rep.hist_bin_ms}</text>`; });
  const ax = x0 + (rep.live.avg_reaction_ms - rep.hist_start_ms) / rep.hist_bin_ms * bw;
  s += `<path d="M${ax} 22V${y0}" stroke="#ff3b30" stroke-width="1.5"/><text x="${ax + 5}" y="30" style="fill:#ff3b30">avg ${Math.round(rep.live.avg_reaction_ms)} ms</text>`;
  $('#hist').innerHTML = s;
}

function polar(d) {
  const cx = 114, cy = 112, R0 = 74, max = Math.max(...d, 1) * 1.1;
  let s = '';
  [.33, .66, 1].forEach(k => s += `<circle cx="${cx}" cy="${cy}" r="${R0 * k}" fill="none" stroke="rgba(255,255,255,${k === 1 ? .2 : .07})"/>`);
  for (let i = 0; i < 8; i++) { const a = i * 45 * DEG; s += `<path d="M${cx} ${cy}L${cx + Math.cos(a) * R0} ${cy + Math.sin(a) * R0}" stroke="rgba(255,255,255,.06)"/>`; }
  d.forEach((v, i) => {
    const r = v / max * R0, a0 = (i * 45 - 20) * DEG, a1 = (i * 45 + 20) * DEG;
    s += `<path d="M${cx} ${cy}L${cx + Math.cos(a0) * r} ${cy + Math.sin(a0) * r}A${r} ${r} 0 0 1 ${cx + Math.cos(a1) * r} ${cy + Math.sin(a1) * r}Z" fill="rgba(255,59,48,${v === Math.max(...d) ? .85 : .35})" stroke="#ff3b30" stroke-width="1"/>`;
  });
  [['Right', cx + R0 + 4, cy + 3, 'start'], ['Left', cx - R0 - 4, cy + 3, 'end'], ['Up', cx, cy - R0 - 6, 'middle'], ['Down', cx, cy + R0 + 14, 'middle']].forEach(([l, x, y, a]) => s += `<text x="${x}" y="${y}" text-anchor="${a}">${l}</text>`);
  s += `<text x="${cx + 4}" y="${cy - R0 * .66 - 3}">${fx(max * .66, 0)}%</text>`;
  const worst = d.indexOf(Math.max(...d)), names = ['right', 'down-right', 'down', 'down-left', 'left', 'up-left', 'up', 'up-right'];
  s += `<text x="114" y="226" text-anchor="middle" style="fill:rgba(255,255,255,.64)">Most overshoot: ${names[worst]} (${fx(d[worst], 1)}%)</text>`;
  $('#polar').innerHTML = s;
}

// ---------- feel match ----------
let F = null;
const SC = { a: { accuracy: .94, avg_ttk_ms: 418, overshoot_pct: 4.1 }, b: { accuracy: .9, avg_ttk_ms: 446, overshoot_pct: 2.6 } };
async function feelStart(demo) {
  const base = S.res ? S.res.cm360_from : cm360(G(S.from.game).yaw, S.from.sens, S.from.dpi);
  F = await api('feel_start', { config: { base_cm360: base, spread: 0.35, tolerance_pct: 4, max_rounds: 8 } });
  if (demo) for (const c of ['b', 'a', 'same', 'a']) F = await api('feel_submit', { choice: c });
  renderFeel();
}
function feelEq(cm) { const g = G(S.from.game), t = G(S.to.game), d = +S.from.dpi; return `${g.code} ${sensFor(g, cm, d).toFixed(g.dec)} · ${t.code} ${sensFor(t, cm, d).toFixed(t.dec)}`; }

function rangeSvg(s) {
  const el = $('#f-svg'), W = el.clientWidth || 640, c = s.config, L0 = c.base_cm360 * (1 - c.spread), H0 = c.base_cm360 * (1 + c.spread);
  el.setAttribute('viewBox', `0 0 ${W} 270`);
  const X = v => 34 + Math.log(v / L0) / Math.log(H0 / L0) * (W - 140);
  let o = `<path d="M34 22H${W - 106}" stroke="rgba(255,255,255,.2)"/>`;
  for (let v = Math.ceil(L0); v <= H0; v++) { const big = v % 5 === 0; o += `<path d="M${X(v)} 22v${big ? -8 : -4}" stroke="rgba(255,255,255,${big ? .45 : .16})"/>`; if (big) o += `<text x="${X(v)}" y="9" text-anchor="middle">${v}</text>`; }
  s.rounds.forEach((r, i) => {
    const y = 44 + i * 28, live = i === s.rounds.length - 1 && !s.done, m = ll(r.lo, r.hi, .5);
    o += `<text x="0" y="${y + 9}">R${r.index}</text><rect x="${X(r.lo)}" y="${y}" width="${X(r.hi) - X(r.lo)}" height="10" rx="2" fill="${live ? '#fff' : 'rgba(255,255,255,.1)'}"/>`;
    if (r.choice) { const [a, b] = r.choice === 'a' ? [r.lo, m] : r.choice === 'b' ? [m, r.hi] : [r.a_cm360, r.b_cm360]; o += `<rect x="${X(a)}" y="${y}" width="${X(b) - X(a)}" height="10" rx="2" fill="rgba(255,255,255,.42)"/>`; }
    o += `<path d="M${X(r.a_cm360)} ${y - 3}v16M${X(r.b_cm360)} ${y - 3}v16" stroke="${live ? '#ff3b30' : 'rgba(255,255,255,.4)'}" stroke-width="${live ? 2 : 1}"/>`;
    if (live) o += `<text x="${X(r.a_cm360)}" y="${y - 6}" text-anchor="middle" style="fill:#ff3b30">A</text><text x="${X(r.b_cm360)}" y="${y - 6}" text-anchor="middle" style="fill:#ff3b30">B</text>`;
    o += `<text x="${W}" y="${y + 9}" text-anchor="end" style="fill:rgba(255,255,255,${live ? .9 : .4})">${r.choice ? { a: 'A', b: 'B', same: 'same' }[r.choice] : 'now'} · ±${fx((r.hi - r.lo) / 2 / m * 100, 1)}%</text>`;
  });
  for (let i = s.rounds.length; i < c.max_rounds; i++) { const y = 44 + i * 28; o += `<text x="0" y="${y + 9}" style="fill:rgba(255,255,255,.18)">R${i + 1}</text><path d="M34 ${y + 5.5}H${W - 106}" stroke="rgba(255,255,255,.07)" stroke-dasharray="2 4"/>`; }
  const m = ll(s.lo, s.hi, .5);
  o += `<path d="M${X(m)} 26V${40 + s.rounds.length * 28}" stroke="#ff3b30" stroke-dasharray="2 3"/>`;
  el.innerHTML = o;
}

function renderFeel() {
  const s = F; if (!s) return;
  const r = s.rounds.at(-1), m = ll(s.lo, s.hi, .5), unc = (s.hi - s.lo) / 2 / m * 100, n = s.rounds.filter(x => x.choice).length;
  $('#f-round').textContent = s.done ? `Done · ${n} rounds` : `Round ${r.index} of ${s.config.max_rounds}`;
  $('#f-width').textContent = `${fx(s.lo, 1)} – ${fx(s.hi, 1)} cm · ±${fx(unc, 1)}%`;
  rangeSvg(s);
  const cand = (k, cm, tag) => { const sc = (s.sc || SC)[k]; return `<div class="panel cand${s.pick === k ? ' pick' : ''}"><div class="ph"><span class="letter">${k.toUpperCase()}</span><span class="lbl">${tag}</span></div>
    <div class="huge"><span>${fx(cm, 1)}</span><small>cm / 360°</small></div><div class="eq">${feelEq(cm)}</div>
    <button class="btn" data-run="${k}"><svg class="ic"><use href="#i-play"/></svg>Run 12 flicks</button>
    <div class="runs"><span>acc ${fx(sc.accuracy * 100, 0)}%</span><span>TTK ${Math.round(sc.avg_ttk_ms)} ms</span><span>over ${fx(sc.overshoot_pct, 1)}%</span></div></div>`; };
  $('#f-ab').innerHTML = s.done ? '' : cand('a', r.a_cm360, 'Faster') + `<div class="choice"><button class="btn" data-c="a">A felt better<kbd>A</kbd></button><button class="btn" data-c="same">Same<kbd>S</kbd></button><button class="btn" data-c="b">B felt better<kbd>B</kbd></button></div>` + cand('b', r.b_cm360, 'Slower');
  const L0 = s.config.base_cm360 * (1 - s.config.spread), H0 = s.config.base_cm360 * (1 + s.config.spread), d = +S.from.dpi;
  $('#f-est').innerHTML = `<span class="lbl">${s.done ? 'Your cm/360' : 'Current estimate'}</span><div class="huge"><span>${fx(m, 1)}</span><small>±${fx(unc, 1)}%</small></div>
    <div class="bar"><i style="left:${(Math.log(s.lo / L0) / Math.log(H0 / L0) * 100).toFixed(2)}%;right:${(100 - Math.log(s.hi / L0) / Math.log(H0 / L0) * 100).toFixed(2)}%"></i></div>
    <div class="sm t3" style="font-size:12px;margin-bottom:14px">${n} of ${s.config.max_rounds} answers · stops at ±${s.config.tolerance_pct / 2}%</div>
    <span class="lbl">Equivalent at ${d} DPI</span>
    <table class="eqt" style="margin:6px 0 14px">${GAMES.map(g => `<tr><td>${g.name}</td><td>${sensFor(g, m, d).toFixed(g.dec)}</td></tr>`).join('')}</table>
    <button class="btn primary" id="b-apply" style="width:100%;justify-content:center"><svg class="ic"><use href="#i-save"/></svg>Save as profile</button>`;
}

// ---------- routing & events ----------
function route() {
  const r = (location.hash.slice(1) || 'convert').split('?')[0];
  $$('.view').forEach(v => v.classList.toggle('on', v.id === 'v-' + r));
  $$('.nav').forEach(n => n.classList.toggle('on', n.dataset.r === r));
  if (r === 'aim') { sizeArena(); renderHud(); drawArena(); }
  if (r === 'results') renderResults();
  if (r === 'feel') renderFeel();
  if (r === 'convert' && S.res) showResult(S.res), update();
}

document.addEventListener('click', async e => {
  const el = e.target.closest('button, .prof'); if (!el) return;
  const d = el.dataset;
  if (d.win && tauri) return tauri.window.getCurrentWindow()[d.win]();
  if (d.g) { S[d.side].game = d.g; S[d.side].fov = null; return renderConvert(); }
  if (d.mi) { S.mi = +d.mi; return renderConvert(); }
  if (d.mode) { A.mode = d.mode; return renderHud(); }
  if (d.p) { const p = S.profiles.find(x => x.id === +d.p); Object.assign(S.from, { game: p.game, sens: p.sens, dpi: p.dpi, fov: p.fov }); $$('.prof').forEach(x => x.classList.toggle('on', x === el)); location.hash = '#convert'; return renderConvert(); }
  if (d.c) { F = await api('feel_submit', { choice: d.c }).catch(err => (toast(String(err)), F)); F.pick = null; return renderFeel(); }
  if (d.run) {
    // TODO(owner): run a 12-target flick set in the arena at this cm/360 and send real scores.
    const rr = lrng(Date.now()), sc = { accuracy: .84 + rr() * .14, avg_ttk_ms: 390 + rr() * 90, overshoot_pct: 1 + rr() * 8 };
    F.sc = { ...(F.sc || SC), [d.run]: sc };
    const pick = await api('feel_scores', { a: F.sc.a, b: F.sc.b }); F.pick = pick; renderFeel();
    return toast(pick ? `Scores favour ${pick === 'same' ? 'neither side' : pick.toUpperCase()}` : 'Run the other side too');
  }
  switch (el.id) {
    case 'b-swap': Object.assign(S, { from: { ...S.to, sens: S.res.sens }, to: { game: S.from.game, dpi: S.from.dpi, fov: S.from.fov } }); return renderConvert();
    case 'b-import': try { S.from.game = 'cs2'; S.from.sens = await api('import_cs2'); toast(`Imported CS2 sensitivity ${S.from.sens}`); renderConvert(); } catch (err) { toast(String(err)); } return;
    case 'b-verify': { const p = await api('verify_plan', { game: S.to.game, sens: S.res.sens, dpi: +S.to.dpi || +S.from.dpi }); return toast(`360 verify: ${fk(p.counts)} counts = ${fx(p.cm)} cm of travel for one full turn`); }
    case 'b-save': case 'b-add': case 'b-rsave': case 'b-apply': {
      const est = el.id === 'b-apply' && F ? ll(F.lo, F.hi, .5) : null, g = est ? G(S.from.game) : G(S.to.game);
      const sens = est ? sensFor(g, est, +S.from.dpi) : S.res.sens;
      await api('profile_save', { profile: { id: 0, name: '', created: 0, game: g.id, sens, dpi: +S.to.dpi || +S.from.dpi, fov: null, cm360: 0, source: est ? 'feel' : 'convert', note: METHODS[S.mi][1] } });
      toast('Profile saved'); return renderSide();
    }
    case 'b-copy': navigator.clipboard?.writeText($('#cfg-line').innerText); return toast('Config line copied');
    case 'b-aim': return A.running ? aimEnd() : aimStart();
    case 'b-retry': location.hash = '#aim'; return;
    case 'b-undo': F = await api('feel_undo'); return renderFeel();
    case 'b-restart': return feelStart(false);
  }
});

document.addEventListener('input', e => {
  const t = e.target;
  if (t.id === 'in-sens') S.from.sens = t.value;
  else if (t.dataset.k) S[t.dataset.side][t.dataset.k] = t.value === '' ? null : +t.value;
  else return;
  update();
});

document.addEventListener('keydown', e => {
  if (e.target.tagName === 'INPUT') return;
  const v = (location.hash.slice(1) || 'convert');
  if (e.key === 'Escape' && A.running) return aimEnd();
  if (v === 'feel' && F && !F.done && 'asb'.includes(e.key.toLowerCase()) && e.key.length === 1) { const c = { a: 'a', s: 'same', b: 'b' }[e.key.toLowerCase()]; return api('feel_submit', { choice: c }).then(x => { F = x; renderFeel(); }); }
  const n = ['1', '2', '3', '4'].indexOf(e.key); if (n >= 0 && !A.running) location.hash = '#' + ['convert', 'aim', 'results', 'feel'][n];
});

$('#arena').addEventListener('mousemove', aimMove);
$('#arena').addEventListener('mousedown', aimShoot);
window.addEventListener('hashchange', route);
window.addEventListener('resize', () => { if (A.w) { sizeArena(); drawArena(); } });
if (tauri) tauri.event.listen('game-detected', ev => setDetect(ev.payload));

window.SM = { S, A, G, GAMES, METHODS, get F() { return F; }, set F(v) { F = v; }, get R() { return R; }, renderConvert, showResult, convertLocal, request, drawArena, sizeArena, renderHud, renderStats, renderFeel, renderResults, ll };
(async () => { await renderConvert(); await renderSide(); await feelStart(!tauri); route(); })();
