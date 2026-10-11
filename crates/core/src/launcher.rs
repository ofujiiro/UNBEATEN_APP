//! Everything the desktop app and the CLI do, behind one type: settings,
//! detected paths, sign-in, the ranked-map manifest, map installs and sync.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::api::{
    ApiClient, ApiError, LinkPoll, LinkStart, Manifest, ManifestMap, Me, ServerScore, SyncTotals,
};
use crate::auth_store::TokenStore;
use crate::charts::{scan_custom_songs, ChartCache, LocalFolder};
use crate::maps::{install_map, map_status, InstallError, InstallOutcome, MapState, MapStatus};
use crate::paths::{self, Detected, CUSTOM_SONGS_DIR};
use crate::process::GameWatch;
use crate::save::{read_save, SaveEntry, SaveError};
use crate::settings::Settings;
use crate::sync::{sync_once, SyncReport, SyncState};

const MANIFEST_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("sign in first")]
    NotSignedIn,
    #[error("UNBEATABLE's save file wasn't found. Set it in Settings.")]
    NoSaveFile,
    #[error("UNBEATABLE's CustomSongs folder wasn't found. Set it in Settings.")]
    NoCustomSongs,
    #[error("that map isn't ranked")]
    UnknownMap,
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Save(#[from] SaveError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl LauncherError {
    /// The server no longer accepts this launcher's token.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, LauncherError::Api(ApiError::Unauthorized))
    }
}

pub type Result<T> = std::result::Result<T, LauncherError>;

/// Where the launcher keeps its own files.
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl AppDirs {
    /// The platform's usual folders (`%APPDATA%`, `~/.config`, `~/.local/share`).
    pub fn standard() -> Self {
        let name = "UNBEATEN Launcher";
        AppDirs {
            config: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(name),
            data: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(name),
        }
    }

    pub fn single(dir: &Path) -> Self {
        AppDirs {
            config: dir.to_path_buf(),
            data: dir.to_path_buf(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub save_file: Option<PathBuf>,
    pub custom_songs: Option<PathBuf>,
    pub game_dir: Option<PathBuf>,
    pub detected: Detected,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapView {
    #[serde(flatten)]
    pub map: ManifestMap,
    pub status: MapStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreView {
    pub song: String,
    pub folder: String,
    pub tier: String,
    pub mode: String,
    pub score: i64,
    pub accuracy: f64,
    pub max_combo: i64,
    pub misses: i64,
    pub modifier_mask: i64,
    pub cleared: bool,
    /// Last sync outcome; `None` if never synced.
    pub status: Option<String>,
    pub pp: Option<f64>,
    pub verified: bool,
    pub beatmap_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub server: String,
    pub signed_in: bool,
    pub me: Option<Me>,
    pub settings: Settings,
    pub paths: Paths,
    pub last_sync_at: Option<i64>,
    pub last_totals: Option<SyncTotals>,
    pub version: &'static str,
}

/// A prepared map install (see `Launcher::prepare_install`).
#[derive(Clone)]
pub struct InstallJob {
    pub api: ApiClient,
    pub map: ManifestMap,
    pub custom_songs: PathBuf,
    /// Folder name under CustomSongs to install into.
    pub target: String,
}

impl InstallJob {
    pub fn run(
        &self,
        progress: impl FnMut(u64, Option<u64>),
    ) -> std::result::Result<InstallOutcome, InstallError> {
        install_map(
            &self.api,
            &self.map,
            &self.custom_songs,
            &self.target,
            progress,
        )
    }
}

pub struct Launcher {
    pub dirs: AppDirs,
    pub settings: Settings,
    detected: Detected,
    tokens: TokenStore,
    token: Option<String>,
    me: Option<Me>,
    manifest: Option<Manifest>,
    manifest_etag: Option<String>,
    manifest_at: Option<Instant>,
    state: SyncState,
    cache: ChartCache,
    save_stamp: Option<(u64, SystemTime)>,
    game: GameWatch,
}

pub fn device_name() -> String {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "This computer".into());
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "linux" => "Linux",
        "macos" => "macOS",
        other => other,
    };
    format!("{host} ({os})")
}

impl Launcher {
    pub fn open(dirs: AppDirs, tokens: TokenStore) -> Self {
        let _ = fs::create_dir_all(&dirs.config);
        let _ = fs::create_dir_all(&dirs.data);
        let settings = Settings::load(&dirs.config);
        let token = tokens.get(&settings.server_url);
        let state = SyncState::load(&dirs.data, &settings.server_url, None);
        Launcher {
            detected: paths::detect(&paths::home_dir()),
            settings,
            tokens,
            token,
            me: None,
            manifest: None,
            manifest_etag: None,
            manifest_at: None,
            state,
            cache: ChartCache::default(),
            save_stamp: None,
            game: GameWatch::new(),
            dirs,
        }
    }

    pub fn server(&self) -> &str {
        &self.settings.server_url
    }

    pub fn api(&self) -> ApiClient {
        ApiClient::new(&self.settings.server_url, self.token.clone())
    }

    pub fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    pub fn me(&self) -> Option<&Me> {
        self.me.as_ref()
    }

    pub fn redetect(&mut self) {
        self.detected = paths::detect(&paths::home_dir());
        self.save_stamp = None;
    }

    pub fn save_file(&self) -> Option<PathBuf> {
        self.settings
            .save_file
            .clone()
            .or_else(|| self.detected.save_file.clone())
    }

    /// The CustomSongs folder: the override, the detected one, or where it
    /// should be inside the game's data folder.
    pub fn custom_songs(&self) -> Option<PathBuf> {
        self.settings
            .custom_songs
            .clone()
            .or_else(|| self.detected.custom_songs.clone())
            .or_else(|| {
                self.detected
                    .data_dirs
                    .first()
                    .map(|d| d.join(CUSTOM_SONGS_DIR))
            })
            .or_else(|| {
                self.save_file()
                    .and_then(|s| s.parent().map(|p| p.join(CUSTOM_SONGS_DIR)))
            })
    }

    pub fn paths(&self) -> Paths {
        Paths {
            save_file: self.save_file(),
            custom_songs: self.custom_songs(),
            game_dir: self.detected.game_dir.clone(),
            detected: self.detected.clone(),
        }
    }

    pub fn update_settings(&mut self, settings: Settings) -> Result<()> {
        let server_changed = settings.server_url != self.settings.server_url;
        let paths_changed = settings.save_file != self.settings.save_file
            || settings.custom_songs != self.settings.custom_songs;
        self.settings = settings;
        self.settings.save(&self.dirs.config)?;
        if server_changed {
            self.token = self.tokens.get(&self.settings.server_url);
            self.me = None;
            self.manifest = None;
            self.manifest_etag = None;
            self.manifest_at = None;
            self.state = SyncState::load(&self.dirs.data, &self.settings.server_url, None);
        }
        if paths_changed {
            self.save_stamp = None;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            server: self.settings.server_url.clone(),
            signed_in: self.signed_in(),
            me: self.me.clone(),
            settings: self.settings.clone(),
            paths: self.paths(),
            last_sync_at: self.state.last_sync_at,
            last_totals: self.state.last_totals.clone(),
            version: crate::api::CLIENT_VERSION,
        }
    }

    // ── Sign-in ──────────────────────────────────────────────────────────────

    pub fn start_link(&self) -> Result<LinkStart> {
        Ok(ApiClient::new(&self.settings.server_url, None).link_start(&device_name())?)
    }

    /// Poll once; on approval the token is stored and the profile loaded.
    pub fn poll_link(&mut self, device_code: &str) -> Result<LinkPoll> {
        let poll = ApiClient::new(&self.settings.server_url, None).link_poll(device_code)?;
        if let LinkPoll::Approved { token, .. } = &poll {
            self.tokens.set(&self.settings.server_url, token)?;
            self.token = Some(token.clone());
            self.refresh_me()?;
        }
        Ok(poll)
    }

    /// Use a token obtained elsewhere (the CLI's `--token-file`).
    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn sign_out(&mut self) {
        if self.token.is_some() {
            let _ = self.api().logout();
        }
        self.forget_token();
    }

    fn forget_token(&mut self) {
        self.tokens.delete(&self.settings.server_url);
        self.token = None;
        self.me = None;
    }

    fn guard<T>(&mut self, r: std::result::Result<T, ApiError>) -> Result<T> {
        if matches!(r, Err(ApiError::Unauthorized)) {
            self.forget_token();
        }
        Ok(r?)
    }

    pub fn refresh_me(&mut self) -> Result<Me> {
        if self.token.is_none() {
            return Err(LauncherError::NotSignedIn);
        }
        let r = self.api().me();
        let me = self.guard(r)?;
        if self.state.user_id != Some(me.id) {
            // Claims a state not tied to a player yet; another player's starts fresh.
            if self.state.user_id.is_some() {
                self.state =
                    SyncState::load(&self.dirs.data, &self.settings.server_url, Some(me.id));
            }
            self.state.user_id = Some(me.id);
        }
        self.me = Some(me.clone());
        Ok(me)
    }

    // ── Maps ─────────────────────────────────────────────────────────────────

    pub fn refresh_manifest(&mut self, force: bool) -> Result<()> {
        if !force
            && self.manifest.is_some()
            && self
                .manifest_at
                .is_some_and(|t| t.elapsed() < MANIFEST_MAX_AGE)
        {
            return Ok(());
        }
        let etag = if self.manifest.is_some() {
            self.manifest_etag.as_deref()
        } else {
            None
        };
        if let Some((m, tag)) = self.api().manifest(etag)? {
            self.manifest = Some(m);
            self.manifest_etag = tag;
        }
        self.manifest_at = Some(Instant::now());
        Ok(())
    }

    pub fn local_folders(&mut self) -> Vec<LocalFolder> {
        match self.custom_songs() {
            Some(dir) => scan_custom_songs(&dir, &mut self.cache),
            None => Vec::new(),
        }
    }

    pub fn maps(&mut self) -> Result<Vec<MapView>> {
        self.refresh_manifest(false)?;
        let folders = self.local_folders();
        let maps = self
            .manifest
            .as_ref()
            .map(|m| m.beatmaps.clone())
            .unwrap_or_default();
        Ok(maps
            .into_iter()
            .map(|map| {
                let status = map_status(&map, &folders);
                MapView { map, status }
            })
            .collect())
    }

    pub fn game_running(&mut self) -> bool {
        self.game.is_running()
    }

    /// What an install needs, so the download can run without holding the
    /// launcher (see `finish_install`).
    pub fn prepare_install(&mut self, beatmap_id: i64) -> Result<InstallJob> {
        if self.game_running() {
            return Err(InstallError::GameRunning.into());
        }
        self.refresh_manifest(false)?;
        let map = self
            .manifest
            .as_ref()
            .and_then(|m| m.beatmaps.iter().find(|b| b.id == beatmap_id))
            .cloned()
            .ok_or(LauncherError::UnknownMap)?;
        let dir = self.custom_songs().ok_or(LauncherError::NoCustomSongs)?;
        // Update an existing copy where it is, under the player's folder name.
        let folders = self.local_folders();
        let target = map_status(&map, &folders)
            .local_folder
            .unwrap_or_else(|| map.install_name().to_string());
        Ok(InstallJob {
            api: self.api(),
            map,
            custom_songs: dir,
            target,
        })
    }

    /// After an install: the folder's existing save records belong to the old
    /// chart, so they're baselined.
    pub fn finish_install(&mut self, job: &InstallJob) -> Result<()> {
        if let Some(save) = self.save_file() {
            if let Ok(entries) = read_save(&save) {
                self.state.baseline_folder(&entries, &job.target);
            }
        }
        let path = job.custom_songs.join(&job.target);
        let charts = crate::charts::scan::scan_folder(&path, &mut self.cache);
        self.state.remember_folder(&LocalFolder {
            name: job.target.clone(),
            path,
            charts,
        });
        self.state.save(&self.dirs.data)?;
        Ok(())
    }

    /// Install one map from start to finish (CLI).
    pub fn install(&mut self, beatmap_id: i64) -> Result<InstallOutcome> {
        let job = self.prepare_install(beatmap_id)?;
        let outcome = job.run(|_, _| {})?;
        self.finish_install(&job)?;
        Ok(outcome)
    }

    /// Ids of ranked maps that are outdated here (for "update all").
    pub fn outdated_maps(&mut self) -> Result<Vec<i64>> {
        Ok(self
            .maps()?
            .into_iter()
            .filter(|m| m.status.state == MapState::Outdated)
            .map(|m| m.map.id)
            .collect())
    }

    // ── Scores ───────────────────────────────────────────────────────────────

    /// Has the save file changed since the last call? (cheap: size and mtime)
    pub fn save_changed(&mut self) -> bool {
        let stamp = self
            .save_file()
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        let changed = stamp != self.save_stamp;
        self.save_stamp = stamp;
        changed
    }

    pub fn read_entries(&self) -> Result<Vec<SaveEntry>> {
        let path = self.save_file().ok_or(LauncherError::NoSaveFile)?;
        Ok(read_save(&path)?)
    }

    pub fn sync(&mut self) -> Result<SyncReport> {
        if self.token.is_none() {
            return Err(LauncherError::NotSignedIn);
        }
        let entries = self.read_entries()?;
        let folders = self.local_folders();
        if let Err(e) = self.refresh_manifest(false) {
            log::warn!("manifest refresh failed: {e}");
        }
        let etag = self.manifest_etag.clone();
        let api = self.api();
        let r = sync_once(&api, &entries, &folders, &mut self.state, etag.as_deref());
        // Save even on failure: local outcomes (baselines, failed runs) count.
        self.state.save(&self.dirs.data)?;
        let report = self.guard(r)?;
        if let (Some(me), Some(t)) = (self.me.as_mut(), &report.totals) {
            me.total_pp = t.total_pp;
            me.rank = t.rank;
        }
        Ok(report)
    }

    pub fn scores(&self) -> Result<Vec<ScoreView>> {
        let entries = self.read_entries()?;
        Ok(entries
            .iter()
            .filter_map(|e| {
                let (folder, tier) = e.custom_key()?;
                let mode = e
                    .song
                    .rsplit('\\')
                    .next()
                    .filter(|m| *m != e.song)
                    .unwrap_or("")
                    .to_string();
                let s = self
                    .state
                    .entries
                    .get(&e.song)
                    .filter(|s| s.fingerprint == e.fingerprint());
                Some(ScoreView {
                    song: e.song.clone(),
                    folder,
                    tier,
                    mode,
                    score: e.score(),
                    accuracy: e.accuracy(),
                    max_combo: e.max_combo(),
                    misses: e.miss_count(),
                    modifier_mask: e.modifier_mask(),
                    cleared: e.cleared(),
                    status: s.map(|s| s.status.clone()),
                    pp: s.and_then(|s| s.pp),
                    verified: s.is_some_and(|s| s.verified),
                    beatmap_id: s.and_then(|s| s.beatmap_id),
                })
            })
            .collect())
    }

    pub fn server_scores(&mut self) -> Result<Vec<ServerScore>> {
        let r = self.api().scores();
        self.guard(r)
    }
}
