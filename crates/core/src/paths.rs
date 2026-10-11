//! Where UNBEATABLE keeps its data: `arcade-highscores.json` and `CustomSongs/`.
//!
//! Unity's persistent data folder, `<company>/<product>` = `D-CELL GAMES/UNBEATABLE`:
//! - Windows: `%USERPROFILE%\AppData\LocalLow\D-CELL GAMES\UNBEATABLE`
//! - Linux, Windows build under Proton (the usual case): the same path inside the
//!   game's prefix, `<library>/steamapps/compatdata/2240620/pfx/drive_c/users/steamuser/AppData/LocalLow/…`
//! - Linux, native build: `~/.config/unity3d/D-CELL GAMES/UNBEATABLE`
//!
//! Folder names are matched without regard to case. The save file is found by
//! a bounded search inside the data folder (it may sit in a profile subfolder),
//! newest first. Settings can override both paths.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::steam;

pub const SAVE_FILE_NAME: &str = "arcade-highscores.json";
pub const CUSTOM_SONGS_DIR: &str = "CustomSongs";
const SAVE_SEARCH_DEPTH: usize = 4;

fn child_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        e.file_name()
            .to_str()
            .filter(|n| n.eq_ignore_ascii_case(name))
            .map(|_| e.path())
    })
}

/// `<base>/D-CELL GAMES/UNBEATABLE`, matching names without regard to case.
fn game_data_under(base: &Path) -> Option<PathBuf> {
    let company = child_ci(base, "D-CELL GAMES")?;
    let product = child_ci(&company, "UNBEATABLE")?;
    product.is_dir().then_some(product)
}

/// Folders that may hold `D-CELL GAMES/UNBEATABLE`, most likely first.
pub fn data_dir_bases(home: &Path, libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.push(home.join("AppData").join("LocalLow"));
    for lib in libraries {
        out.push(
            lib.join("steamapps")
                .join("compatdata")
                .join(steam::UNBEATABLE_APP_ID)
                .join("pfx/drive_c/users/steamuser/AppData/LocalLow"),
        );
    }
    out.push(home.join(".config").join("unity3d"));
    out
}

/// Every existing UNBEATABLE data folder.
pub fn find_data_dirs(home: &Path, libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for base in data_dir_bases(home, libraries) {
        if let Some(dir) = game_data_under(&base) {
            if !out.contains(&dir) {
                out.push(dir);
            }
        }
    }
    out
}

fn mtime(p: &Path) -> SystemTime {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Every `arcade-highscores.json` under `dir` (depth-limited, skipping
/// `CustomSongs`), newest first.
pub fn find_saves(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for e in read.flatten() {
            let path = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_file() && name.eq_ignore_ascii_case(SAVE_FILE_NAME) {
                out.push(path);
            } else if kind.is_dir()
                && depth > 0
                && !name.eq_ignore_ascii_case(CUSTOM_SONGS_DIR)
                && !name.starts_with('.')
            {
                walk(&path, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, SAVE_SEARCH_DEPTH, &mut out);
    out.sort_by_key(|p| std::cmp::Reverse(mtime(p)));
    out
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Detected {
    pub steam_roots: Vec<PathBuf>,
    pub game_dir: Option<PathBuf>,
    /// UNBEATABLE data folders found, the chosen one first.
    pub data_dirs: Vec<PathBuf>,
    pub save_file: Option<PathBuf>,
    /// Other save files found, in case the newest isn't the right one.
    pub other_saves: Vec<PathBuf>,
    pub custom_songs: Option<PathBuf>,
    /// The data folder exists but has no CustomSongs folder yet.
    pub custom_songs_missing: bool,
}

/// Look for Steam, the game and its data in the usual places.
pub fn detect(home: &Path) -> Detected {
    let steam_roots = steam::steam_roots(home);
    let mut libraries = Vec::new();
    for root in &steam_roots {
        for lib in steam::libraries(root) {
            if !libraries.contains(&lib) {
                libraries.push(lib);
            }
        }
    }
    let mut det = detect_in(home, &libraries);
    det.steam_roots = steam_roots;
    det
}

/// `detect` with the Steam libraries given (for tests and overrides).
pub fn detect_in(home: &Path, libraries: &[PathBuf]) -> Detected {
    let data_dirs = find_data_dirs(home, libraries);
    // Prefer the data folder whose save was written last.
    let mut best: Option<(SystemTime, PathBuf, Vec<PathBuf>)> = None;
    for dir in &data_dirs {
        let saves = find_saves(dir);
        let newest = saves
            .first()
            .map(|p| mtime(p))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _, _)| newest > *t) {
            best = Some((newest, dir.clone(), saves));
        }
    }
    let (data_dir, saves) = match best {
        Some((_, dir, saves)) => (Some(dir), saves),
        None => (None, Vec::new()),
    };
    let custom_songs = data_dir
        .as_ref()
        .and_then(|d| child_ci(d, CUSTOM_SONGS_DIR));
    let mut ordered = data_dirs.clone();
    if let Some(d) = &data_dir {
        ordered.retain(|x| x != d);
        ordered.insert(0, d.clone());
    }
    Detected {
        steam_roots: Vec::new(),
        game_dir: steam::find_game_dir(libraries),
        custom_songs_missing: data_dir.is_some() && custom_songs.is_none(),
        data_dirs: ordered,
        save_file: saves.first().cloned(),
        other_saves: saves.into_iter().skip(1).collect(),
        custom_songs,
    }
}

/// The user's home folder.
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn finds_a_proton_prefix_and_a_profile_save() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let lib = tmp.path().join("lib");
        let data = lib.join(
            "steamapps/compatdata/2240620/pfx/drive_c/users/steamuser/AppData/LocalLow/D-CELL GAMES/UNBEATABLE",
        );
        fs::create_dir_all(data.join("profiles/1")).unwrap();
        fs::create_dir_all(data.join("CustomSongs/Song")).unwrap();
        fs::write(data.join("CustomSongs/Song/arcade-highscores.json"), "{}").unwrap();
        fs::write(data.join("profiles/1").join(SAVE_FILE_NAME), "{}").unwrap();

        let det = detect_in(&home, &[lib]);
        assert_eq!(det.data_dirs, vec![data.clone()]);
        assert_eq!(
            det.save_file,
            Some(data.join("profiles/1").join(SAVE_FILE_NAME))
        );
        assert_eq!(det.custom_songs, Some(data.join("CustomSongs")));
        assert!(!det.custom_songs_missing);
    }

    #[test]
    fn matches_names_without_case_and_prefers_the_newest_save() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let windows = home.join("AppData/LocalLow/D-Cell Games/Unbeatable");
        let native = home.join(".config/unity3d/D-CELL GAMES/UNBEATABLE");
        fs::create_dir_all(&windows).unwrap();
        fs::create_dir_all(&native).unwrap();
        fs::write(windows.join(SAVE_FILE_NAME), "{}").unwrap();
        sleep(Duration::from_millis(20));
        fs::write(native.join(SAVE_FILE_NAME), "{}").unwrap();

        let det = detect_in(&home, &[]);
        assert_eq!(det.data_dirs.len(), 2);
        assert_eq!(det.data_dirs[0], native);
        assert_eq!(det.save_file, Some(native.join(SAVE_FILE_NAME)));
        assert!(det.custom_songs_missing);
    }

    #[test]
    fn nothing_found_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let det = detect_in(tmp.path(), &[]);
        assert!(det.data_dirs.is_empty() && det.save_file.is_none() && det.custom_songs.is_none());
    }
}
