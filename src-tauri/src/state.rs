//! State shared by the window's commands, the tray and the background worker.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use unbeaten_core::api::ApiError;
use unbeaten_core::api::{SyncTotals, UpdateInfo};
use unbeaten_core::sync::{Imported, SyncReport};
use unbeaten_core::util::now_unix;
use unbeaten_core::{Launcher, LauncherError};

const ACTIVITY_KEPT: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    pub at: i64,
    /// `sync`, `install`, `game` or `error`.
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub imported: Vec<Imported>,
    pub totals: Option<SyncTotals>,
}

pub struct AppState {
    pub launcher: Mutex<Launcher>,
    /// Device code of the sign-in in progress, if any.
    pub link: Mutex<Option<String>>,
    pub game_running: AtomicBool,
    pub syncing: AtomicBool,
    pub tray_ok: AtomicBool,
    pub activity: Mutex<VecDeque<Activity>>,
    pub update: Mutex<Option<UpdateInfo>>,
    /// Set when the server says this launcher is too old.
    pub update_required: Mutex<Option<String>>,
}

/// Locks a mutex even if a thread panicked while holding it.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn new(launcher: Launcher) -> Self {
        AppState {
            launcher: Mutex::new(launcher),
            link: Mutex::new(None),
            game_running: AtomicBool::new(false),
            syncing: AtomicBool::new(false),
            tray_ok: AtomicBool::new(false),
            activity: Mutex::new(VecDeque::new()),
            update: Mutex::new(None),
            update_required: Mutex::new(None),
        }
    }

    pub fn launcher(&self) -> MutexGuard<'_, Launcher> {
        lock(&self.launcher)
    }

    pub fn push_activity(&self, app: &AppHandle, a: Activity) {
        {
            let mut list = lock(&self.activity);
            list.push_front(a.clone());
            list.truncate(ACTIVITY_KEPT);
        }
        let _ = app.emit("activity://new", a);
    }

    pub fn note(&self, app: &AppHandle, kind: &str, message: impl Into<String>) {
        self.push_activity(
            app,
            Activity {
                at: now_unix(),
                kind: kind.into(),
                message: message.into(),
                imported: vec![],
                totals: None,
            },
        );
    }

    /// Tell the window to refresh what it shows.
    pub fn changed(&self, app: &AppHandle) {
        let _ = app.emit("state://changed", ());
    }

    fn handle_error(&self, app: &AppHandle, err: &LauncherError) {
        match err {
            LauncherError::Api(ApiError::UpdateRequired(msg)) => {
                *lock(&self.update_required) = Some(msg.clone());
            }
            e if e.is_unauthorized() => {
                self.note(
                    app,
                    "error",
                    "The launcher was signed out. Sign in again to keep syncing.",
                );
            }
            LauncherError::Api(ApiError::Network(_)) => {
                self.note(
                    app,
                    "error",
                    "Couldn't reach UNBEATEN. Scores will sync when it's back.",
                );
            }
            e => self.note(app, "error", format!("Sync failed: {e}")),
        }
    }

    /// Sync now (from the worker, the tray or the window). `None` if a sync
    /// was already running.
    pub fn sync(&self, app: &AppHandle) -> Option<Result<SyncReport, LauncherError>> {
        if self.syncing.swap(true, Ordering::SeqCst) {
            return None;
        }
        self.changed(app);
        let result = self.launcher().sync();
        self.syncing.store(false, Ordering::SeqCst);
        let notify = self.launcher().settings.notifications;
        let out = match result {
            Ok(report) => {
                if report.sent > 0 {
                    self.push_activity(app, sync_activity(&report));
                }
                if notify && !report.imported.is_empty() {
                    let a = sync_activity(&report);
                    let _ = app
                        .notification()
                        .builder()
                        .title("UNBEATEN")
                        .body(a.message)
                        .show();
                }
                Ok(report)
            }
            Err(e) => {
                self.handle_error(app, &e);
                Err(e)
            }
        };
        self.changed(app);
        Some(out)
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn sync_activity(report: &SyncReport) -> Activity {
    let mut parts = Vec::new();
    if !report.imported.is_empty() {
        parts.push(format!(
            "{} synced",
            plural(report.imported.len(), "new score", "new scores")
        ));
    }
    let count = |s: &str| report.statuses.get(s).copied().unwrap_or(0);
    if count("no_change") > 0 {
        parts.push(format!("{} not better than before", count("no_change")));
    }
    if count("outdated_chart") + count("modified_chart") > 0 {
        parts.push(format!(
            "{} on charts that differ from the ranked version",
            count("outdated_chart") + count("modified_chart")
        ));
    }
    if count("unmatched") > 0 {
        parts.push(format!("{} on unranked charts", count("unmatched")));
    }
    let mut message = if parts.is_empty() {
        "Synced.".to_string()
    } else {
        parts.join(" · ")
    };
    if let Some(t) = &report.totals {
        let gained = t.total_pp - t.total_pp_before;
        if gained.abs() >= 0.01 {
            message.push_str(&format!(" — {:+.2}pp", gained));
        }
        if t.rank != t.rank_before {
            message.push_str(&format!(", #{} → #{}", t.rank_before, t.rank));
        }
    }
    Activity {
        at: now_unix(),
        kind: "sync".into(),
        message,
        imported: report.imported.clone(),
        totals: report.totals.clone(),
    }
}
