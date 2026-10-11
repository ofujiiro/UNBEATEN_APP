//! Sending new scores from the save file to UNBEATEN.
//!
//! An entry is sent when its fingerprint differs from the one recorded after
//! it was last handled. Failed runs are recorded without being sent.
//!
//! **Baseline rule.** The save keeps one record per chart until a play beats
//! it, so after a chart changes (an update, a reinstall, a hand edit) the old
//! record still sits there, set on the old chart. Whenever a folder's charts
//! change, the current records of that folder are marked as handled
//! ("baseline") so they aren't sent as if they were played on the new chart.
//! The first time a folder is seen nothing is baselined, so a first sync sends
//! the whole save, like a web import.
//!
//! Entries the server couldn't match are retried when the ranked-map manifest
//! changes (the chart may have been ranked since).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, SyncRequest, SyncTotals, CLIENT_VERSION};
use crate::charts::scan::find_folder;
use crate::charts::{LocalFolder, CHART_HASH_ALGO};
use crate::save::SaveEntry;
use crate::util::{norm, now_unix, read_json_or_default, write_json};

const CHUNK: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryState {
    pub fingerprint: String,
    /// Server status (`imported`, `no_change`, `unmatched`, `outdated_chart`,
    /// `modified_chart`, …) or a local one (`failed_run`, `baseline`).
    pub status: String,
    #[serde(default)]
    pub pp: Option<f64>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub beatmap_id: Option<i64>,
    pub at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    pub server: String,
    pub user_id: Option<i64>,
    /// Song key → what was last done with it.
    pub entries: BTreeMap<String, EntryState>,
    /// norm(folder) → signature of its charts when last seen.
    pub folder_sigs: BTreeMap<String, String>,
    pub manifest_etag: Option<String>,
    pub last_sync_at: Option<i64>,
    pub last_totals: Option<SyncTotals>,
}

impl SyncState {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("sync_state.json")
    }

    /// The state for this server and player; another server or player starts
    /// fresh. A state not yet tied to a player (made before the profile was
    /// loaded, e.g. by an install) is kept and claimed.
    pub fn load(dir: &Path, server: &str, user_id: Option<i64>) -> Self {
        let mut state: SyncState = read_json_or_default(&Self::path(dir));
        let same_player = user_id.is_none() || state.user_id.is_none() || state.user_id == user_id;
        if state.server == server && same_player {
            state.user_id = user_id.or(state.user_id);
            return state;
        }
        SyncState {
            server: server.to_string(),
            user_id,
            ..Default::default()
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        write_json(&Self::path(dir), self)
    }

    fn mark(&mut self, entry: &SaveEntry, status: &str) {
        self.entries.insert(
            entry.song.clone(),
            EntryState {
                fingerprint: entry.fingerprint(),
                status: status.to_string(),
                pp: None,
                verified: false,
                beatmap_id: None,
                at: now_unix(),
            },
        );
    }

    /// Mark the current records of `folder` as handled (see the baseline rule).
    pub fn baseline_folder(&mut self, entries: &[SaveEntry], folder: &str) -> usize {
        let key = norm(folder);
        let mut n = 0;
        for e in entries {
            if e.custom_key().is_some_and(|(f, _)| norm(&f) == key) {
                let unchanged = self
                    .entries
                    .get(&e.song)
                    .is_some_and(|s| s.fingerprint == e.fingerprint());
                if !unchanged {
                    self.mark(e, "baseline");
                    n += 1;
                }
            }
        }
        n
    }

    /// Record a folder's signature without baselining (after an install,
    /// which baselines itself).
    pub fn remember_folder(&mut self, folder: &LocalFolder) {
        self.folder_sigs
            .insert(norm(&folder.name), folder.signature());
    }
}

#[derive(Debug, Default)]
pub struct Plan<'a> {
    pub to_send: Vec<&'a SaveEntry>,
    pub baselined: usize,
    pub failed_runs: usize,
}

/// Decide what to send, updating `state` for local outcomes (baselines,
/// failed runs, unmatched entries to retry).
pub fn plan<'a>(
    entries: &'a [SaveEntry],
    folders: &[LocalFolder],
    state: &mut SyncState,
    manifest_etag: Option<&str>,
) -> Plan<'a> {
    let mut plan = Plan::default();

    for folder in folders {
        let key = norm(&folder.name);
        let sig = folder.signature();
        match state.folder_sigs.get(&key) {
            Some(prev) if *prev != sig => {
                plan.baselined += state.baseline_folder(entries, &folder.name);
            }
            _ => {}
        }
        state.folder_sigs.insert(key, sig);
    }

    if let Some(tag) = manifest_etag {
        if state.manifest_etag.as_deref().is_some_and(|old| old != tag) {
            state.entries.retain(|_, s| s.status != "unmatched");
        }
        state.manifest_etag = Some(tag.to_string());
    }

    for e in entries.iter().filter(|e| e.is_custom()) {
        let fp = e.fingerprint();
        if state
            .entries
            .get(&e.song)
            .is_some_and(|s| s.fingerprint == fp)
        {
            continue;
        }
        if !e.cleared() {
            state.mark(e, "failed_run");
            plan.failed_runs += 1;
            continue;
        }
        plan.to_send.push(e);
    }
    plan
}

#[derive(Debug, Clone, Serialize)]
pub struct Imported {
    pub song: String,
    pub beatmap_id: Option<i64>,
    pub pp: f64,
    pub previous_pp: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub sent: usize,
    pub imported: Vec<Imported>,
    /// Count per status, for everything sent.
    pub statuses: BTreeMap<String, usize>,
    pub baselined: usize,
    pub failed_runs: usize,
    pub totals: Option<SyncTotals>,
}

/// One sync: plan, send in chunks, record the results in `state`.
pub fn sync_once(
    api: &ApiClient,
    entries: &[SaveEntry],
    folders: &[LocalFolder],
    state: &mut SyncState,
    manifest_etag: Option<&str>,
) -> Result<SyncReport, ApiError> {
    let plan = plan(entries, folders, state, manifest_etag);
    let mut report = SyncReport {
        baselined: plan.baselined,
        failed_runs: plan.failed_runs,
        ..Default::default()
    };
    let mut first_totals: Option<SyncTotals> = None;
    let mut last_totals: Option<SyncTotals> = None;

    for chunk in plan.to_send.chunks(CHUNK) {
        let mut local = BTreeMap::new();
        for e in chunk {
            if let Some((folder, _)) = e.custom_key() {
                if let Some(f) = find_folder(folders, &folder) {
                    local.insert(f.name.clone(), f.charts.clone());
                }
            }
        }
        let req = SyncRequest {
            client_version: CLIENT_VERSION,
            hash_algo: CHART_HASH_ALGO,
            entries: chunk.to_vec(),
            folders: local,
        };
        let resp = api.sync(&req)?;
        let now = now_unix();
        for r in resp.results {
            let Some(e) = chunk.get(r.i) else { continue };
            *report.statuses.entry(r.status.clone()).or_default() += 1;
            if r.status == "imported" {
                report.imported.push(Imported {
                    song: e.song.clone(),
                    beatmap_id: r.beatmap_id,
                    pp: r.pp.unwrap_or(0.0),
                    previous_pp: r.previous_pp,
                });
            }
            state.entries.insert(
                e.song.clone(),
                EntryState {
                    fingerprint: e.fingerprint(),
                    status: r.status,
                    pp: r.pp,
                    verified: r.verified,
                    beatmap_id: r.beatmap_id,
                    at: now,
                },
            );
        }
        report.sent += chunk.len();
        first_totals.get_or_insert_with(|| resp.user.clone());
        last_totals = Some(resp.user);
    }

    if let (Some(first), Some(last)) = (first_totals, last_totals) {
        let totals = SyncTotals {
            total_pp_before: first.total_pp_before,
            total_pp: last.total_pp,
            rank_before: first.rank_before,
            rank: last.rank,
        };
        state.last_totals = Some(totals.clone());
        report.totals = Some(totals);
    }
    state.last_sync_at = Some(now_unix());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::LocalChart;
    use crate::save::parse_save;

    fn entries(json: &str) -> Vec<SaveEntry> {
        parse_save(json.as_bytes()).unwrap()
    }

    fn folder(name: &str, hash: &str) -> LocalFolder {
        LocalFolder {
            name: name.into(),
            path: PathBuf::from(name),
            charts: vec![LocalChart {
                file: "a.txt".into(),
                hash: hash.into(),
            }],
        }
    }

    const SAVE: &str = r#"{"highScores":[
        {"song":"CUSTOM_Song/Hard\\Classic","score":1,"cleared":true,"updateCount":0},
        {"song":"CUSTOM_Song/Easy\\Classic","score":2,"cleared":false},
        {"song":"CUSTOM_Other/Hard\\Classic","score":3,"cleared":true},
        {"song":"FOREVER NOW/UNBEATABLE\\Classic","score":4,"cleared":true}
    ]}"#;

    #[test]
    fn first_run_sends_every_cleared_custom_entry() {
        let e = entries(SAVE);
        let mut state = SyncState::default();
        let p = plan(&e, &[folder("Song", "h1")], &mut state, None);
        let songs: Vec<_> = p.to_send.iter().map(|e| e.song.as_str()).collect();
        assert_eq!(
            songs,
            ["CUSTOM_Song/Hard\\Classic", "CUSTOM_Other/Hard\\Classic"]
        );
        assert_eq!((p.failed_runs, p.baselined), (1, 0));
        assert_eq!(
            state.entries["CUSTOM_Song/Easy\\Classic"].status,
            "failed_run"
        );
    }

    #[test]
    fn handled_entries_wait_for_a_change() {
        let e = entries(SAVE);
        let mut state = SyncState::default();
        for x in plan(&e, &[], &mut state, None).to_send {
            state.mark(x, "imported");
        }
        assert!(plan(&e, &[], &mut state, None).to_send.is_empty());

        let better = SAVE.replace(
            r#""score":1,"cleared":true,"updateCount":0"#,
            r#""score":9,"cleared":true,"updateCount":1"#,
        );
        let e2 = entries(&better);
        let songs: Vec<_> = plan(&e2, &[], &mut state, None)
            .to_send
            .iter()
            .map(|e| e.song.clone())
            .collect();
        assert_eq!(songs, ["CUSTOM_Song/Hard\\Classic"]);
    }

    #[test]
    fn a_changed_chart_baselines_its_folder() {
        let e = entries(SAVE);
        let mut state = SyncState::default();
        let _ = plan(
            &e,
            &[folder("Song", "v1"), folder("Other", "o")],
            &mut state,
            None,
        );
        // Nothing was sent yet (say the server was down), then Song was updated.
        let p = plan(
            &e,
            &[folder("Song", "v2"), folder("Other", "o")],
            &mut state,
            None,
        );
        let songs: Vec<_> = p.to_send.iter().map(|e| e.song.as_str()).collect();
        assert_eq!(songs, ["CUSTOM_Other/Hard\\Classic"]);
        assert_eq!(
            state.entries["CUSTOM_Song/Hard\\Classic"].status,
            "baseline"
        );
        assert_eq!(p.baselined, 1); // the failed run was already recorded
    }

    #[test]
    fn unmatched_entries_retry_when_the_manifest_changes() {
        let e = entries(SAVE);
        let mut state = SyncState::default();
        for x in plan(&e, &[], &mut state, Some("\"a\"")).to_send {
            state.mark(x, "unmatched");
        }
        assert!(plan(&e, &[], &mut state, Some("\"a\"")).to_send.is_empty());
        assert_eq!(plan(&e, &[], &mut state, Some("\"b\"")).to_send.len(), 2);
    }

    #[test]
    fn state_is_per_server_and_player() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = SyncState::load(dir.path(), "https://a", Some(1));
        s.folder_sigs.insert("x".into(), "y".into());
        s.save(dir.path()).unwrap();
        assert_eq!(
            SyncState::load(dir.path(), "https://a", Some(1))
                .folder_sigs
                .len(),
            1
        );
        assert!(SyncState::load(dir.path(), "https://a", Some(2))
            .folder_sigs
            .is_empty());
        assert!(SyncState::load(dir.path(), "https://b", Some(1))
            .folder_sigs
            .is_empty());
    }

    #[test]
    fn a_state_made_before_sign_in_is_claimed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = SyncState::load(dir.path(), "https://a", None);
        s.folder_sigs.insert("x".into(), "y".into());
        s.save(dir.path()).unwrap();
        let claimed = SyncState::load(dir.path(), "https://a", Some(5));
        assert_eq!((claimed.user_id, claimed.folder_sigs.len()), (Some(5), 1));
    }
}
