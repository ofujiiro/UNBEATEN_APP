//! UNBEATEN Launcher: the desktop app. The work is done by `unbeaten-core`;
//! this crate adds the window, the tray and the background worker.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;
mod tray;
mod worker;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};
use unbeaten_core::auth_store::TokenStore;
use unbeaten_core::{AppDirs, Launcher};

use crate::state::AppState;

fn main() {
    // Started by the OS at login: stay in the tray.
    let minimized = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app)
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(move |app| {
            // Same folders as the `ubl` CLI, so both share settings and sync state.
            let dirs = AppDirs::standard();
            let launcher = Launcher::open(dirs.clone(), TokenStore::new(&dirs.config));
            let hidden = minimized || launcher.settings.start_minimized;
            let state = Arc::new(AppState::new(launcher));
            app.manage(state.clone());

            if let Err(e) = tray::create(app.handle()) {
                log::warn!("no tray icon: {e}");
            }
            worker::start(app.handle().clone(), state.clone());

            let tray = state.tray_ok.load(std::sync::atomic::Ordering::SeqCst);
            if !hidden || !tray {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<AppState>>();
                let to_tray = state.launcher().settings.close_to_tray;
                if to_tray && state.tray_ok.load(std::sync::atomic::Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::auth_begin,
            commands::auth_cancel,
            commands::sign_out,
            commands::redetect,
            commands::save_settings,
            commands::maps_list,
            commands::map_install,
            commands::maps_update_all,
            commands::scores_list,
            commands::sync_now,
            commands::launch_game,
            commands::open_external,
            commands::reveal_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running UNBEATEN Launcher");
}
