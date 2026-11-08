//! Shared application state and the background game watcher.

use crate::aim::{AimEngine, AimReport};
use crate::feel::FeelSession;
use crate::games;
use crate::history::{self, HistoryStore};
use crate::profiles::{self, ProfileStore};
use crate::verify::RawCounter;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter};

pub struct AppState {
    pub profiles: Mutex<ProfileStore>,
    pub feel: Mutex<Option<FeelSession>>,
    pub aim: Mutex<Option<AimEngine>>,
    pub last_report: Mutex<Option<AimReport>>,
    pub history: Mutex<HistoryStore>,
    pub raw: Mutex<RawCounter>,
    /// Id of the supported game currently running, if any.
    pub running_game: Arc<Mutex<Option<&'static str>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            profiles: Mutex::new(ProfileStore::open(profiles::default_path())),
            feel: Mutex::new(None),
            aim: Mutex::new(None),
            last_report: Mutex::new(None),
            history: Mutex::new(HistoryStore::open(history::default_path())),
            raw: Mutex::new(RawCounter::default()),
            running_game: Arc::new(Mutex::new(None)),
        }
    }
}

/// Lock that survives a poisoned mutex (a panicking command must not brick the app).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Scan the process list for a supported game.
pub fn detect_running_game(sys: &mut System) -> Option<&'static str> {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes()
        .values()
        .find_map(|p| games::by_process(&p.name().to_string_lossy()).map(|g| g.id))
}

/// Poll every 5 s and emit `game-detected` when the running game changes.
/// Only reads process names; nothing is injected or hooked.
pub fn spawn_game_watcher(app: AppHandle, slot: Arc<Mutex<Option<&'static str>>>) {
    let spawned = thread::Builder::new().name("game-watcher".into()).spawn(move || {
        let mut sys = System::new_with_specifics(RefreshKind::nothing());
        loop {
            let found = detect_running_game(&mut sys);
            let changed = {
                let mut cur = lock(&slot);
                let changed = *cur != found;
                *cur = found;
                changed
            };
            if changed {
                log::info!("running game: {:?}", found);
                let _ = app.emit("game-detected", found);
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
    if let Err(e) = spawned {
        log::warn!("game watcher not started: {e}");
    }
}
