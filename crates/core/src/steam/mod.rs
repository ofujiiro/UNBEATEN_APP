//! Finding Steam, its libraries and UNBEATABLE.

pub mod vdf;

use std::fs;
use std::path::{Path, PathBuf};

pub const UNBEATABLE_APP_ID: &str = "2240620";

/// Places Steam is usually installed, for the current platform.
pub fn steam_root_candidates(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        out.extend(windows_registry_steam_roots());
        out.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        out.push(PathBuf::from(r"C:\Program Files\Steam"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        for rel in [
            ".steam/steam",
            ".steam/root",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ".var/app/com.valvesoftware.Steam/data/Steam",
            "snap/steam/common/.local/share/Steam",
            "Library/Application Support/Steam",
        ] {
            out.push(home.join(rel));
        }
    }
    let _ = home;
    out
}

#[cfg(target_os = "windows")]
fn windows_registry_steam_roots() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut out = Vec::new();
    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
        if let Ok(p) = key.get_value::<String, _>("SteamPath") {
            out.push(PathBuf::from(p.replace('/', "\\")));
        }
    }
    for sub in [r"SOFTWARE\WOW6432Node\Valve\Steam", r"SOFTWARE\Valve\Steam"] {
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) {
            if let Ok(p) = key.get_value::<String, _>("InstallPath") {
                out.push(PathBuf::from(p));
            }
        }
    }
    out
}

fn dedupe_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        if !p.is_dir() {
            continue;
        }
        let canon = fs::canonicalize(&p).unwrap_or(p);
        if !out.contains(&canon) {
            out.push(canon);
        }
    }
    out
}

/// Existing Steam installs.
pub fn steam_roots(home: &Path) -> Vec<PathBuf> {
    dedupe_existing(steam_root_candidates(home))
}

/// Library folders listed by a Steam install, plus the install itself.
pub fn libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut out = vec![steam_root.to_path_buf()];
    for file in [
        steam_root.join("steamapps").join("libraryfolders.vdf"),
        steam_root.join("config").join("libraryfolders.vdf"),
    ] {
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        let root = vdf::parse(&src);
        let Some(folders) = root.get("libraryfolders") else {
            continue;
        };
        for (_, entry) in folders.entries() {
            let path = match entry {
                vdf::Vdf::Str(p) => Some(p.as_str()),
                obj => obj.get("path").and_then(vdf::Vdf::as_str),
            };
            if let Some(p) = path {
                out.push(PathBuf::from(p));
            }
        }
    }
    dedupe_existing(out)
}

/// Every library of every Steam install found.
pub fn all_libraries(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in steam_roots(home) {
        for lib in libraries(&root) {
            if !out.contains(&lib) {
                out.push(lib);
            }
        }
    }
    out
}

/// UNBEATABLE's install folder in one of `libraries`, from its app manifest.
pub fn find_game_dir(libraries: &[PathBuf]) -> Option<PathBuf> {
    for lib in libraries {
        let manifest = lib
            .join("steamapps")
            .join(format!("appmanifest_{UNBEATABLE_APP_ID}.acf"));
        let Ok(src) = fs::read_to_string(&manifest) else {
            continue;
        };
        let root = vdf::parse(&src);
        let dir = root
            .get("AppState")
            .and_then(|s| s.get("installdir"))
            .and_then(vdf::Vdf::as_str)
            .unwrap_or("UNBEATABLE");
        let game = lib.join("steamapps").join("common").join(dir);
        if game.is_dir() {
            return Some(game);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_game_in_a_second_library() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Steam");
        let lib2 = tmp.path().join("Games").join("SteamLibrary");
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::create_dir_all(lib2.join("steamapps/common/UNBEATABLE")).unwrap();
        fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" \"apps\" {{ \"2240620\" \"1\" }} }} }}",
                root.display().to_string().replace('\\', "\\\\"),
                lib2.display().to_string().replace('\\', "\\\\"),
            ),
        )
        .unwrap();
        fs::write(
            lib2.join("steamapps/appmanifest_2240620.acf"),
            "\"AppState\" { \"appid\" \"2240620\" \"installdir\" \"UNBEATABLE\" }",
        )
        .unwrap();

        let libs = libraries(&root);
        assert_eq!(libs.len(), 2);
        assert_eq!(
            find_game_dir(&libs).map(|p| fs::canonicalize(p).unwrap()),
            Some(fs::canonicalize(lib2.join("steamapps/common/UNBEATABLE")).unwrap())
        );
    }
}
