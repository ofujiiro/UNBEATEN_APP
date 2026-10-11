//! Installing or updating a ranked map in `CustomSongs`.
//!
//! The ZIP is downloaded and unpacked in `UNBEATEN Launcher/staging`, next to
//! `CustomSongs` (never inside it, where the game would see a half-written
//! song), checked against the manifest's chart hashes, then renamed into
//! place. A folder it replaces is moved to `UNBEATEN Launcher/backups`, not
//! deleted. Nothing outside the target folder is touched.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::api::{ApiClient, ApiError, ManifestMap};
use crate::charts::scan::scan_folder;
use crate::charts::ChartCache;
use crate::util::now_unix;

const MAX_ENTRIES: usize = 4096;
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
pub const WORK_DIR: &str = "UNBEATEN Launcher";

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("close UNBEATABLE before installing or updating maps")]
    GameRunning,
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("the download isn't a valid map: {0}")]
    BadArchive(String),
    #[error("the downloaded charts don't match the ranked version; try again later")]
    Verify,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallOutcome {
    pub folder: PathBuf,
    /// Where the replaced folder went, if there was one.
    pub backup: Option<PathBuf>,
}

/// A single folder name, safe to join under `CustomSongs`.
pub fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.ends_with([' ', '.']);
    ok.then_some(name)
}

/// Unpack `zip` into `dest`, which must end up holding exactly `folder/…`.
pub fn extract_map_zip(zip: &Path, dest: &Path, folder: &str) -> Result<PathBuf, InstallError> {
    let bad = |m: &str| InstallError::BadArchive(m.to_string());
    let mut archive = zip::ZipArchive::new(File::open(zip)?).map_err(|e| bad(&e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(bad("too many files"));
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| bad(&e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(bad("unsafe path in archive"));
        };
        if entry.is_symlink() {
            return Err(bad("links aren't allowed"));
        }
        let parts: Vec<_> = rel.components().collect();
        let Some(Component::Normal(top)) = parts.first() else {
            return Err(bad("unsafe path in archive"));
        };
        if top.to_string_lossy() == "__MACOSX" {
            continue;
        }
        if *top != folder {
            return Err(bad("unexpected folder in archive"));
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&out)?;
        let limit = MAX_UNPACKED_BYTES.saturating_sub(total) + 1;
        let written = io::copy(&mut (&mut entry).take(limit), &mut file)?;
        total += written;
        if total > MAX_UNPACKED_BYTES {
            return Err(bad("archive too large"));
        }
    }
    let root = dest.join(folder);
    if !root.is_dir() {
        return Err(bad("the map folder is missing"));
    }
    Ok(root)
}

/// Download `map` and put it in `custom_songs/<target>`. `target` is the
/// existing local copy being updated (keeping the player's folder name, and
/// so their save keys), or `map.install_name()` for a fresh install.
pub fn install_map(
    api: &ApiClient,
    map: &ManifestMap,
    custom_songs: &Path,
    target: &str,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<InstallOutcome, InstallError> {
    let folder = safe_name(map.install_name())
        .ok_or_else(|| InstallError::BadArchive("bad folder name".into()))?;
    let target_name =
        safe_name(target).ok_or_else(|| InstallError::BadArchive("bad folder name".into()))?;
    fs::create_dir_all(custom_songs)?;
    let work = custom_songs.parent().unwrap_or(custom_songs).join(WORK_DIR);
    let staging = work
        .join("staging")
        .join(format!("{}-{}", map.id, now_unix()));
    fs::create_dir_all(&staging)?;
    let result = (|| {
        let zip = staging.join("download.zip");
        api.download(&map.download_url, &zip, progress)?;
        let unpacked = extract_map_zip(&zip, &staging.join("unpacked"), folder)?;

        let mut cache = ChartCache::default();
        let local: Vec<String> = scan_folder(&unpacked, &mut cache)
            .into_iter()
            .map(|c| c.hash)
            .collect();
        if !map.current_hashes().all(|h| local.iter().any(|l| l == h)) {
            return Err(InstallError::Verify);
        }

        let target = custom_songs.join(target_name);
        let mut backup = None;
        if target.exists() {
            let dir = work.join("backups");
            fs::create_dir_all(&dir)?;
            let b = dir.join(format!("{target_name}-{}", now_unix()));
            fs::rename(&target, &b)?;
            backup = Some(b);
        }
        if let Err(e) = fs::rename(&unpacked, &target) {
            if let Some(b) = &backup {
                let _ = fs::rename(b, &target);
            }
            return Err(e.into());
        }
        Ok(InstallOutcome {
            folder: target,
            backup,
        })
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zw = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            zw.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn extracts_the_expected_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("m.zip");
        make_zip(
            &zip,
            &[
                ("Song/cover.png", b"png"),
                ("Song/a [Hard].txt", b"x"),
                ("__MACOSX/Song/._a", b"junk"),
            ],
        );
        let root = extract_map_zip(&zip, &dir.path().join("out"), "Song").unwrap();
        assert!(root.join("a [Hard].txt").is_file());
        assert!(!dir.path().join("out/__MACOSX").exists());
    }

    #[test]
    fn rejects_traversal_and_other_folders() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("evil.zip");
        make_zip(&zip, &[("Song/../../escape.txt", b"x")]);
        assert!(matches!(
            extract_map_zip(&zip, &dir.path().join("out"), "Song"),
            Err(InstallError::BadArchive(_))
        ));
        assert!(!dir.path().join("escape.txt").exists());

        let zip = dir.path().join("other.zip");
        make_zip(&zip, &[("Other/a.txt", b"x")]);
        assert!(matches!(
            extract_map_zip(&zip, &dir.path().join("out2"), "Song"),
            Err(InstallError::BadArchive(_))
        ));
    }

    #[test]
    fn folder_names_are_checked() {
        assert!(safe_name("Flavor Foley - Static (Uluxite)").is_some());
        for bad in ["", ".", "..", "a/b", "a\\b", "trailing."] {
            assert!(safe_name(bad).is_none(), "{bad}");
        }
    }
}
