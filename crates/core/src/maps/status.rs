//! How a ranked map stands on this computer, by comparing chart hashes.

use std::collections::HashSet;

use serde::Serialize;

use crate::api::ManifestMap;
use crate::charts::scan::find_folder;
use crate::charts::LocalFolder;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapState {
    /// Every current chart is there, unchanged.
    Installed,
    /// The folder holds an older version of the map.
    Outdated,
    /// The folder's charts match no ranked version (edited, or another chart
    /// with the same folder name).
    Modified,
    NotInstalled,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapStatus {
    pub state: MapState,
    /// The local folder the state refers to.
    pub local_folder: Option<String>,
    /// That folder isn't named like the ranked one (a renamed copy).
    pub renamed: bool,
}

fn judge(map: &ManifestMap, folder: &LocalFolder) -> MapState {
    let local: HashSet<&str> = folder.hashes().collect();
    let current: Vec<&str> = map.current_hashes().collect();
    if current.is_empty() {
        // The server couldn't hash this map; a same-named folder is the best we know.
        return MapState::Installed;
    }
    if current.iter().all(|h| local.contains(h)) {
        MapState::Installed
    } else if map
        .previous_hashes
        .iter()
        .any(|h| local.contains(h.as_str()))
    {
        MapState::Outdated
    } else {
        MapState::Modified
    }
}

pub fn map_status(map: &ManifestMap, folders: &[LocalFolder]) -> MapStatus {
    let named =
        find_folder(folders, map.install_name()).or_else(|| find_folder(folders, &map.folder_name));
    let named_state = named.map(|f| judge(map, f));
    if named_state == Some(MapState::Installed) {
        return MapStatus {
            state: MapState::Installed,
            local_folder: named.map(|f| f.name.clone()),
            renamed: false,
        };
    }

    // A renamed copy only counts if its charts prove it's this map.
    let hashes: HashSet<&str> = map
        .current_hashes()
        .chain(map.previous_hashes.iter().map(String::as_str))
        .collect();
    let renamed: Vec<(&LocalFolder, MapState)> = folders
        .iter()
        .filter(|f| Some(f.name.as_str()) != named.map(|n| n.name.as_str()))
        .filter(|f| f.hashes().any(|h| hashes.contains(h)))
        .map(|f| (f, judge(map, f)))
        .collect();
    if let Some((f, _)) = renamed.iter().find(|(_, s)| *s == MapState::Installed) {
        return MapStatus {
            state: MapState::Installed,
            local_folder: Some(f.name.clone()),
            renamed: true,
        };
    }
    if let (Some(f), Some(state)) = (named, named_state) {
        return MapStatus {
            state,
            local_folder: Some(f.name.clone()),
            renamed: false,
        };
    }
    if let Some((f, _)) = renamed.iter().find(|(_, s)| *s == MapState::Outdated) {
        return MapStatus {
            state: MapState::Outdated,
            local_folder: Some(f.name.clone()),
            renamed: true,
        };
    }
    MapStatus {
        state: MapState::NotInstalled,
        local_folder: None,
        renamed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ManifestDiff;
    use crate::charts::LocalChart;
    use std::path::PathBuf;

    fn map(current: &[&str], previous: &[&str]) -> ManifestMap {
        ManifestMap {
            id: 1,
            folder_name: "Song".into(),
            install_folder: None,
            title: None,
            artist: None,
            creator: None,
            version: 2,
            cover_url: None,
            preview_url: None,
            download_url: "/d".into(),
            unchartable_url: None,
            difficulties: current
                .iter()
                .enumerate()
                .map(|(i, h)| ManifestDiff {
                    id: i as i64,
                    name: format!("d{i}"),
                    level: 0,
                    star_rating: 0.0,
                    object_count: 0,
                    max_pp: 0.0,
                    filename: String::new(),
                    chart_hash: Some(h.to_string()),
                })
                .collect(),
            previous_hashes: previous.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn folder(name: &str, hashes: &[&str]) -> LocalFolder {
        LocalFolder {
            name: name.into(),
            path: PathBuf::from(name),
            charts: hashes
                .iter()
                .map(|h| LocalChart {
                    file: format!("{h}.txt"),
                    hash: h.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn states() {
        let m = map(&["a2", "b"], &["a1"]);
        assert_eq!(
            map_status(&m, &[folder("Song", &["a2", "b"])]).state,
            MapState::Installed
        );
        assert_eq!(
            map_status(&m, &[folder("song", &["a2", "b", "extra"])]).state,
            MapState::Installed
        );
        assert_eq!(
            map_status(&m, &[folder("Song", &["a1", "b"])]).state,
            MapState::Outdated
        );
        assert_eq!(
            map_status(&m, &[folder("Song", &["zzz"])]).state,
            MapState::Modified
        );
        assert_eq!(
            map_status(&m, &[folder("Other", &["zzz"])]).state,
            MapState::NotInstalled
        );
    }

    #[test]
    fn renamed_copies_are_recognised_by_hash() {
        let m = map(&["a2", "b"], &["a1"]);
        let s = map_status(&m, &[folder("Song - Copy", &["a2", "b"])]);
        assert_eq!(
            (s.state, s.renamed, s.local_folder.as_deref()),
            (MapState::Installed, true, Some("Song - Copy"))
        );
        let s = map_status(
            &m,
            &[folder("Song", &["zzz"]), folder("Song (old)", &["a1"])],
        );
        assert_eq!(
            (s.state, s.local_folder.as_deref()),
            (MapState::Modified, Some("Song"))
        );
        let s = map_status(&m, &[folder("Song (old)", &["a1", "b"])]);
        assert_eq!((s.state, s.renamed), (MapState::Outdated, true));
    }

    #[test]
    fn the_install_folder_name_counts_as_named() {
        let mut m = map(&["a2"], &[]);
        m.folder_name = "Static _Uluxite_".into();
        m.install_folder = Some("Static (Uluxite)".into());
        let s = map_status(&m, &[folder("Static (Uluxite)", &["zzz"])]);
        assert_eq!((s.state, s.renamed), (MapState::Modified, false));
    }
}
