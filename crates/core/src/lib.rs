//! Core of the UNBEATEN launcher: finds UNBEATABLE and its data, hashes charts
//! like the UNBEATEN server (`ubh1`), installs ranked maps and syncs scores.
//! Used by the desktop app (`src-tauri`) and the `ubl` CLI.

pub mod api;
pub mod auth_store;
pub mod charts;
pub mod launcher;
pub mod maps;
pub mod paths;
pub mod process;
pub mod save;
pub mod settings;
pub mod steam;
pub mod sync;
pub mod util;

pub use launcher::{AppDirs, InstallJob, Launcher, LauncherError};
