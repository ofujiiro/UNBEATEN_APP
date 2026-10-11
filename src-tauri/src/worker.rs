//! Background loop: watches the save file and the game process, syncs after
//! plays, refreshes the profile and checks for launcher updates.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use unbeaten_core::api::{check_for_update, ApiError};
use unbeaten_core::process::GameWatch;
use unbeaten_core::LauncherError;

use crate::state::{lock, AppState};

const TICK: Duration = Duration::from_secs(1);
const GAME_CHECK: Duration = Duration::from_secs(4);
/// The game may write the save more than once; wait for it to settle.
const SAVE_SETTLE: Duration = Duration::from_millis(1500);
const AFTER_GAME_EXIT: Duration = Duration::from_secs(3);
const PROFILE_REFRESH: Duration = Duration::from_secs(600);
const RETRY: Duration = Duration::from_secs(120);

/// Worth trying again later: the server was unreachable or failing.
fn retryable(e: &LauncherError) -> bool {
    matches!(e, LauncherError::Api(ApiError::Network(_)))
        || matches!(e, LauncherError::Api(ApiError::Status(code, _)) if *code >= 500 || *code == 429)
}

pub fn start(app: AppHandle, state: Arc<AppState>) {
    {
        let state = state.clone();
        spawn(move || {
            *lock(&state.update) = check_for_update();
        });
    }
    spawn(move || run(app, state));
}

fn run(app: AppHandle, state: Arc<AppState>) {
    let mut game = GameWatch::new();
    let mut last_game_check: Option<Instant> = None;
    let mut last_profile: Option<Instant> = None;
    let mut sync_at: Option<Instant> = None;

    loop {
        let now = Instant::now();

        if last_game_check.is_none_or(|t| now - t >= GAME_CHECK) {
            last_game_check = Some(now);
            let running = game.is_running();
            let was = state.game_running.swap(running, Ordering::SeqCst);
            if running != was {
                let _ = app.emit("game://state", running);
                state.note(
                    &app,
                    "game",
                    if running {
                        "UNBEATABLE started."
                    } else {
                        "UNBEATABLE closed."
                    },
                );
                if !running {
                    sync_at = Some(now + AFTER_GAME_EXIT);
                }
                state.changed(&app);
            }
        }

        let (signed_in, auto_sync) = {
            let l = state.launcher();
            (l.signed_in(), l.settings.auto_sync)
        };

        if signed_in && last_profile.is_none_or(|t| now - t >= PROFILE_REFRESH) {
            last_profile = Some(now);
            let mut l = state.launcher();
            if l.refresh_me().is_ok() {
                let _ = l.refresh_manifest(true);
            }
            drop(l);
            state.changed(&app);
        }

        if signed_in && auto_sync {
            if state.launcher().save_changed() {
                sync_at = Some(now + SAVE_SETTLE);
            }
            if sync_at.is_some_and(|t| now >= t) {
                sync_at = None;
                match state.sync(&app) {
                    Some(Err(e)) if retryable(&e) => sync_at = Some(now + RETRY),
                    Some(Err(e)) => log::warn!("sync failed: {e}"),
                    _ => {}
                }
            }
        }

        sleep(TICK);
    }
}
