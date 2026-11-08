#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! SensMatch: sensitivity converter between games with a built-in aim test.

mod aim;
mod commands;
mod convert;
mod error;
mod feel;
mod games;
mod history;
mod profiles;
mod state;
mod verify;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_games,
            commands::convert,
            commands::equivalents,
            commands::feel_start,
            commands::feel_submit,
            commands::feel_scores,
            commands::feel_undo,
            commands::aim_start,
            commands::aim_track,
            commands::aim_shoot,
            commands::aim_finish,
            commands::aim_report,
            commands::sessions_list,
            commands::history_trend,
            commands::verify_plan,
            commands::verify_evaluate,
            commands::verify_dpi,
            commands::raw_arm,
            commands::raw_read,
            commands::system_status,
            commands::profiles_list,
            commands::profile_save,
            commands::profile_delete,
            commands::export_config,
            commands::import_cs2,
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show SensMatch", true, None::<&str>)?;
            let aim = MenuItem::with_id(app, "aim", "Start aim test", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &aim, &quit])?;
            let mut tray = TrayIconBuilder::with_id("sensmatch-tray")
                .tooltip("SensMatch")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    id @ ("show" | "aim") => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                            if id == "aim" {
                                let _ = w.eval("location.hash = '#aim'");
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let slot = app.state::<state::AppState>().running_game.clone();
            state::spawn_game_watcher(app.handle().clone(), slot);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SensMatch");
}
