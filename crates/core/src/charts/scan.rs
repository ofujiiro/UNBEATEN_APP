//! Reading the game's `CustomSongs` folder: one folder per song, chart `.txt`
//! files directly inside it.
//!
//! Only top-level `*.txt` files count, which is how UNBEATEN reads a chart ZIP
//! too: `README.txt`, editor autosaves (`*.txt.auto`), dotfiles and subfolders
//! such as `old/` are skipped, and so is any `.txt` without hit objects.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use super::hash::{chart_hash, sha256_hex};
use crate::util::norm;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalChart {
    pub file: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalFolder {
    /// Directory name, which the game uses in save keys (`CUSTOM_<name>/…`).
    pub name: String,
    pub path: PathBuf,
    pub charts: Vec<LocalChart>,
}

impl LocalFolder {
    /// Stable fingerprint of the folder's charts, to notice when they change.
    pub fn signature(&self) -> String {
        let mut hashes: Vec<&str> = self.charts.iter().map(|c| c.hash.as_str()).collect();
        hashes.sort_unstable();
        sha256_hex(hashes.join("\n").as_bytes())
    }

    pub fn hashes(&self) -> impl Iterator<Item = &str> {
        self.charts.iter().map(|c| c.hash.as_str())
    }
}

/// Remembers hashes by (path, size, mtime) so rescanning is cheap.
#[derive(Default)]
pub struct ChartCache {
    entries: HashMap<PathBuf, (u64, Option<SystemTime>, Option<String>)>,
}

impl ChartCache {
    fn hash_file(&mut self, path: &Path) -> Option<String> {
        let meta = fs::metadata(path).ok()?;
        let key = (meta.len(), meta.modified().ok());
        if let Some((len, mtime, hash)) = self.entries.get(path) {
            if (*len, *mtime) == key {
                return hash.clone();
            }
        }
        let hash = fs::read(path).ok().and_then(|data| chart_hash(&data));
        self.entries
            .insert(path.to_path_buf(), (key.0, key.1, hash.clone()));
        hash
    }
}

pub fn is_chart_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.starts_with('.') && lower.ends_with(".txt") && lower != "readme.txt"
}

/// Charts of one song folder, sorted by file name.
pub fn scan_folder(dir: &Path, cache: &mut ChartCache) -> Vec<LocalChart> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut charts: Vec<LocalChart> = read
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            if !is_chart_file(&file) {
                return None;
            }
            let hash = cache.hash_file(&e.path())?;
            Some(LocalChart { file, hash })
        })
        .collect();
    charts.sort_by(|a, b| a.file.cmp(&b.file));
    charts
}

/// Every song folder under `CustomSongs`, sorted by name.
pub fn scan_custom_songs(root: &Path, cache: &mut ChartCache) -> Vec<LocalFolder> {
    let Ok(read) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<LocalFolder> = read
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            let charts = scan_folder(&path, cache);
            Some(LocalFolder { name, path, charts })
        })
        .collect();
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    folders
}

/// Find a local folder by the name a save key uses, exactly or ignoring case
/// and whitespace (the server compares the same way).
pub fn find_folder<'a>(folders: &'a [LocalFolder], name: &str) -> Option<&'a LocalFolder> {
    folders.iter().find(|f| f.name == name).or_else(|| {
        let key = norm(name);
        folders.iter().find(|f| norm(&f.name) == key)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: &str = "[Metadata]\nTitle:X\n[HitObjects]\n213,192,1000,1,0\n298,192,1250,1,0\n";

    #[test]
    fn scans_only_top_level_charts() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("Drifting");
        fs::create_dir_all(song.join("old")).unwrap();
        fs::write(song.join("a [Hard].txt"), CHART).unwrap();
        fs::write(song.join("a [Hard].txt.auto"), CHART).unwrap();
        fs::write(song.join("README.txt"), CHART).unwrap();
        fs::write(song.join("notes.txt"), "just words").unwrap();
        fs::write(song.join("old").join("a [Hard].txt"), CHART).unwrap();
        fs::write(song.join("cover.png"), b"png").unwrap();
        fs::create_dir_all(dir.path().join(".hidden")).unwrap();

        let mut cache = ChartCache::default();
        let folders = scan_custom_songs(dir.path(), &mut cache);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Drifting");
        assert_eq!(
            folders[0].charts,
            vec![LocalChart {
                file: "a [Hard].txt".into(),
                hash: chart_hash(CHART.as_bytes()).unwrap()
            }]
        );
        assert!(find_folder(&folders, "drifting").is_some());
        assert!(find_folder(&folders, "Drift").is_none());
    }

    #[test]
    fn signature_ignores_file_order_and_names() {
        let a = LocalFolder {
            name: "A".into(),
            path: PathBuf::new(),
            charts: vec![
                LocalChart {
                    file: "1".into(),
                    hash: "x".into(),
                },
                LocalChart {
                    file: "2".into(),
                    hash: "y".into(),
                },
            ],
        };
        let b = LocalFolder {
            name: "B".into(),
            path: PathBuf::new(),
            charts: vec![
                LocalChart {
                    file: "z".into(),
                    hash: "y".into(),
                },
                LocalChart {
                    file: "w".into(),
                    hash: "x".into(),
                },
            ],
        };
        assert_eq!(a.signature(), b.signature());
    }
}
