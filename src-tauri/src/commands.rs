//! Commands the window calls. Anything that touches the network or the disk
//! runs on a blocking thread so the window stays responsive.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use unbeaten_core::api::{LinkPoll, LinkStart, UpdateInfo};
use unbeaten_core::launcher::{MapView, ScoreView, Snapshot};
use unbeaten_core::maps::InstallOutcome;
use unbeaten_core::settings::Settings;
use unbeaten_core::steam::UNBEATABLE_APP_ID;
use unbeaten_core::sync::SyncReport;

use crate::state::{lock, Activity, AppState};

type Shared<'a> = State<'a, Arc<AppState>>;

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct AppSnapshot {
    #[serde(flatten)]
    pub core: Snapshot,
    pub game_running: bool,
    pub syncing: bool,
    pub tray: bool,
    pub signing_in: bool,
    pub autostart: bool,
    pub update: Option<UpdateInfo>,
    pub update_required: Option<String>,
    pub activity: Vec<Activity>,
}

fn snapshot(app: &AppHandle, s: &AppState) -> AppSnapshot {
    let core = s.launcher().snapshot();
    AppSnapshot {
        core,
        game_running: s.game_running.load(Ordering::SeqCst),
        syncing: s.syncing.load(Ordering::SeqCst),
        tray: s.tray_ok.load(Ordering::SeqCst),
        signing_in: lock(&s.link).is_some(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        update: lock(&s.update).clone(),
        update_required: lock(&s.update_required).clone(),
        activity: lock(&s.activity).iter().cloned().collect(),
    }
}

#[tauri::command]
pub async fn get_snapshot(app: AppHandle, state: Shared<'_>) -> Result<AppSnapshot, String> {
    let s = state.inner().clone();
    blocking(move || Ok(snapshot(&app, &s))).await
}

// ── Sign-in ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn auth_begin(app: AppHandle, state: Shared<'_>) -> Result<LinkStart, String> {
    let s = state.inner().clone();
    blocking(move || {
        let link = s.launcher().start_link().map_err(|e| e.to_string())?;
        *lock(&s.link) = Some(link.device_code.clone());
        let _ = app
            .opener()
            .open_url(&link.verification_uri_complete, None::<&str>);
        let (app2, s2, link2) = (app.clone(), s.clone(), link.clone());
        spawn(move || poll_link(app2, s2, link2));
        Ok(link)
    })
    .await
}

fn poll_link(app: AppHandle, s: Arc<AppState>, link: LinkStart) {
    let deadline = Instant::now() + Duration::from_secs(link.expires_in);
    let mut interval = Duration::from_secs(link.interval.max(2));
    let still_current = |s: &AppState| lock(&s.link).as_deref() == Some(link.device_code.as_str());
    let finish = |s: &AppState, status: &str| {
        if still_current(s) {
            *lock(&s.link) = None;
        }
        let _ = app.emit("auth://state", json!({ "status": status }));
        s.changed(&app);
    };
    while Instant::now() < deadline {
        sleep(interval);
        if !still_current(&s) {
            return; // cancelled, or a newer sign-in started
        }
        let polled = s.launcher().poll_link(&link.device_code);
        match polled {
            Ok(LinkPoll::Pending) => {}
            Ok(LinkPoll::SlowDown) => interval += Duration::from_secs(1),
            Ok(LinkPoll::Approved { user, .. }) => {
                s.note(&app, "sync", format!("Signed in as {}.", user.username));
                finish(&s, "approved");
                let _ = s.sync(&app);
                return;
            }
            Ok(LinkPoll::Denied) => return finish(&s, "denied"),
            Ok(LinkPoll::Expired) => return finish(&s, "expired"),
            Err(e) => log::warn!("sign-in poll failed: {e}"),
        }
    }
    finish(&s, "expired");
}

#[tauri::command]
pub async fn auth_cancel(app: AppHandle, state: Shared<'_>) -> Result<(), String> {
    *lock(&state.link) = None;
    let _ = app.emit("auth://state", json!({ "status": "cancelled" }));
    Ok(())
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, state: Shared<'_>) -> Result<(), String> {
    let s = state.inner().clone();
    blocking(move || {
        s.launcher().sign_out();
        s.changed(&app);
        Ok(())
    })
    .await
}

// ── Settings and paths ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn redetect(app: AppHandle, state: Shared<'_>) -> Result<AppSnapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        s.launcher().redetect();
        s.changed(&app);
        Ok(snapshot(&app, &s))
    })
    .await
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: Shared<'_>,
    settings: Settings,
    autostart: bool,
) -> Result<AppSnapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        s.launcher()
            .update_settings(settings)
            .map_err(|e| e.to_string())?;
        let auto = app.autolaunch();
        if auto.is_enabled().unwrap_or(false) != autostart {
            let r = if autostart {
                auto.enable()
            } else {
                auto.disable()
            };
            r.map_err(|e| format!("Couldn't change start-up setting: {e}"))?;
        }
        s.changed(&app);
        Ok(snapshot(&app, &s))
    })
    .await
}

// ── Maps ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn maps_list(state: Shared<'_>) -> Result<Vec<MapView>, String> {
    let s = state.inner().clone();
    blocking(move || s.launcher().maps().map_err(|e| e.to_string())).await
}

fn install_one(app: &AppHandle, s: &AppState, id: i64) -> Result<InstallOutcome, String> {
    let job = s
        .launcher()
        .prepare_install(id)
        .map_err(|e| e.to_string())?;
    let title = job
        .map
        .title
        .clone()
        .unwrap_or_else(|| job.map.folder_name.clone());
    let mut last = Instant::now();
    let outcome = job
        .run(|done, total| {
            if last.elapsed() >= Duration::from_millis(150) {
                last = Instant::now();
                let _ = app.emit(
                    "maps://progress",
                    json!({ "id": id, "done": done, "total": total }),
                );
            }
        })
        .map_err(|e| e.to_string())?;
    s.launcher()
        .finish_install(&job)
        .map_err(|e| e.to_string())?;
    let verb = if outcome.backup.is_some() {
        "Updated"
    } else {
        "Installed"
    };
    s.note(app, "install", format!("{verb} {title}."));
    s.changed(app);
    Ok(outcome)
}

#[tauri::command]
pub async fn map_install(
    app: AppHandle,
    state: Shared<'_>,
    id: i64,
) -> Result<InstallOutcome, String> {
    let s = state.inner().clone();
    blocking(move || install_one(&app, &s, id)).await
}

#[tauri::command]
pub async fn maps_update_all(app: AppHandle, state: Shared<'_>) -> Result<usize, String> {
    let s = state.inner().clone();
    blocking(move || {
        let ids = s.launcher().outdated_maps().map_err(|e| e.to_string())?;
        for id in &ids {
            install_one(&app, &s, *id)?;
        }
        Ok(ids.len())
    })
    .await
}

// ── Scores ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn scores_list(state: Shared<'_>) -> Result<Vec<ScoreView>, String> {
    let s = state.inner().clone();
    blocking(move || s.launcher().scores().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn sync_now(app: AppHandle, state: Shared<'_>) -> Result<SyncReport, String> {
    let s = state.inner().clone();
    blocking(move || match s.sync(&app) {
        None => Err("A sync is already running.".into()),
        Some(r) => r.map_err(|e| e.to_string()),
    })
    .await
}

// ── Outside the app ──────────────────────────────────────────────────────────

pub fn open_game(app: &AppHandle) {
    let url = format!("steam://rungameid/{UNBEATABLE_APP_ID}");
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        log::warn!("couldn't start the game through Steam: {e}");
    }
}

#[tauri::command]
pub async fn launch_game(app: AppHandle) -> Result<(), String> {
    open_game(&app);
    Ok(())
}

#[tauri::command]
pub async fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_path(app: AppHandle, path: PathBuf) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}
