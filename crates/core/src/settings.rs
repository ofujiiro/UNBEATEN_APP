//! Launcher settings, saved as JSON in the app's config folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::DEFAULT_SERVER;
use crate::util::{read_json_or_default, write_json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub server_url: String,
    /// Sync automatically when the save file changes.
    pub auto_sync: bool,
    pub start_minimized: bool,
    /// Closing the window keeps the launcher running in the tray.
    pub close_to_tray: bool,
    pub notifications: bool,
    /// Overrides for the detected paths.
    pub save_file: Option<PathBuf>,
    pub custom_songs: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server_url: DEFAULT_SERVER.to_string(),
            auto_sync: true,
            start_minimized: false,
            close_to_tray: true,
            notifications: true,
            save_file: None,
            custom_songs: None,
        }
    }
}

impl Settings {
    fn path(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Self {
        let mut s: Settings = read_json_or_default(&Self::path(dir));
        s.server_url = s.server_url.trim().trim_end_matches('/').to_string();
        if s.server_url.is_empty() {
            s.server_url = DEFAULT_SERVER.to_string();
        }
        s
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        write_json(&Self::path(dir), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let s = Settings {
            server_url: "http://localhost:8081/".into(),
            auto_sync: false,
            ..Default::default()
        };
        s.save(dir.path()).unwrap();
        let back = Settings::load(dir.path());
        assert_eq!(back.server_url, "http://localhost:8081");
        assert!(!back.auto_sync);
    }
}
