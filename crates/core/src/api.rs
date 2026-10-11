//! UNBEATEN's launcher API (`/api/launcher/*`, see UNBEATEN `docs/API.md`).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::charts::LocalChart;
use crate::save::SaveEntry;

pub const DEFAULT_SERVER: &str = "https://unbeaten.cc";
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Largest map download accepted (the server caps uploads at 100 MB).
const MAX_DOWNLOAD_BYTES: u64 = 300 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not signed in, or the launcher was signed out")]
    Unauthorized,
    #[error("this account is banned")]
    Banned,
    #[error("{0}")]
    UpdateRequired(String),
    #[error("server error {0}: {1}")]
    Status(u16, String),
    #[error("can't reach the server: {0}")]
    Network(String),
    #[error("unexpected server response: {0}")]
    Decode(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_decode() {
            ApiError::Decode(e.to_string())
        } else {
            ApiError::Network(e.to_string())
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LinkStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserBrief {
    pub id: i64,
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LinkPoll {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Approved { token: String, user: UserBrief },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Me {
    pub id: i64,
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub total_pp: f64,
    pub rank: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestDiff {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub level: i64,
    #[serde(default)]
    pub star_rating: f64,
    #[serde(default)]
    pub object_count: i64,
    #[serde(default)]
    pub max_pp: f64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub chart_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestMap {
    pub id: i64,
    /// The server's (sanitised) folder name.
    pub folder_name: String,
    /// The folder name to install under: the charter's own when portable.
    #[serde(default)]
    pub install_folder: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub preview_url: Option<String>,
    pub download_url: String,
    #[serde(default)]
    pub unchartable_url: Option<String>,
    #[serde(default)]
    pub difficulties: Vec<ManifestDiff>,
    #[serde(default)]
    pub previous_hashes: Vec<String>,
}

impl ManifestMap {
    /// Folder name for a fresh install (also the download's top folder).
    pub fn install_name(&self) -> &str {
        self.install_folder.as_deref().unwrap_or(&self.folder_name)
    }

    pub fn current_hashes(&self) -> impl Iterator<Item = &str> {
        self.difficulties
            .iter()
            .filter_map(|d| d.chart_hash.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub hash_algo: String,
    #[serde(default)]
    pub min_client_version: String,
    pub beatmaps: Vec<ManifestMap>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerScore {
    pub difficulty_id: i64,
    pub beatmap_id: i64,
    #[serde(default)]
    pub difficulty_name: Option<String>,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub modifier_mask: i64,
    #[serde(default)]
    pub pp: f64,
    #[serde(default)]
    pub accuracy: f64,
    #[serde(default)]
    pub max_combo: i64,
    #[serde(default)]
    pub count_miss: i64,
    #[serde(default)]
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncRequest<'a> {
    pub client_version: &'a str,
    pub hash_algo: &'a str,
    pub entries: Vec<&'a SaveEntry>,
    pub folders: BTreeMap<String, Vec<LocalChart>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyncResult {
    pub i: usize,
    pub song: String,
    pub status: String,
    #[serde(default)]
    pub beatmap_id: Option<i64>,
    #[serde(default)]
    pub difficulty_id: Option<i64>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub pp: Option<f64>,
    #[serde(default)]
    pub previous_pp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SyncTotals {
    pub total_pp_before: f64,
    pub total_pp: f64,
    pub rank_before: i64,
    pub rank: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyncResponse {
    pub results: Vec<SyncResult>,
    pub user: SyncTotals,
}

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    http: Client,
    token: Option<String>,
}

#[derive(Deserialize)]
struct Detail {
    detail: serde_json::Value,
}

fn user_agent() -> String {
    format!(
        "UNBEATEN-Launcher/{CLIENT_VERSION} ({})",
        std::env::consts::OS
    )
}

impl ApiClient {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let http = Client::builder()
            .user_agent(user_agent())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP client");
        ApiClient {
            base: base.trim_end_matches('/').to_string(),
            http,
            token,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Absolute URL for a server path such as `/api/assets/x.png`.
    pub fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base, path)
        }
    }

    fn authed(&self, req: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    fn check(resp: Response) -> Result<Response, ApiError> {
        let status = resp.status();
        if status.is_success() || status == StatusCode::NOT_MODIFIED {
            return Ok(resp);
        }
        let detail = resp
            .json::<Detail>()
            .ok()
            .map(|d| match d.detail {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_default();
        Err(match status.as_u16() {
            401 => ApiError::Unauthorized,
            403 => ApiError::Banned,
            426 => ApiError::UpdateRequired(if detail.is_empty() {
                "This launcher is too old. Please update it.".into()
            } else {
                detail
            }),
            code => ApiError::Status(code, detail),
        })
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, ApiError> {
        let resp = self.authed(self.http.get(self.url(path))).send()?;
        Ok(Self::check(resp)?.json()?)
    }

    fn post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let resp = self
            .authed(self.http.post(self.url(path)))
            .json(body)
            .send()?;
        Ok(Self::check(resp)?.json()?)
    }

    pub fn link_start(&self, device_name: &str) -> Result<LinkStart, ApiError> {
        self.post_json(
            "/api/launcher/link/start",
            &serde_json::json!({ "device_name": device_name, "client_version": CLIENT_VERSION }),
        )
    }

    pub fn link_poll(&self, device_code: &str) -> Result<LinkPoll, ApiError> {
        self.post_json(
            "/api/launcher/link/poll",
            &serde_json::json!({ "device_code": device_code }),
        )
    }

    pub fn me(&self) -> Result<Me, ApiError> {
        self.get_json("/api/launcher/me")
    }

    pub fn logout(&self) -> Result<(), ApiError> {
        let _: serde_json::Value =
            self.post_json("/api/launcher/logout", &serde_json::json!({}))?;
        Ok(())
    }

    pub fn scores(&self) -> Result<Vec<ServerScore>, ApiError> {
        self.get_json("/api/launcher/scores")
    }

    /// The ranked-map manifest; `None` when `etag` is still current.
    pub fn manifest(
        &self,
        etag: Option<&str>,
    ) -> Result<Option<(Manifest, Option<String>)>, ApiError> {
        let mut req = self.http.get(self.url("/api/launcher/manifest"));
        if let Some(tag) = etag {
            req = req.header(reqwest::header::IF_NONE_MATCH, tag);
        }
        let resp = Self::check(req.send()?)?;
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let tag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        Ok(Some((resp.json()?, tag)))
    }

    pub fn sync(&self, req: &SyncRequest<'_>) -> Result<SyncResponse, ApiError> {
        self.post_json("/api/launcher/sync", req)
    }

    /// Download `path` (a server path or URL) into `dest`.
    pub fn download(
        &self,
        path: &str,
        dest: &Path,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), ApiError> {
        let resp = self
            .http
            .get(self.url(path))
            .timeout(Duration::from_secs(600))
            .send()?;
        let mut resp = Self::check(resp)?;
        let total = resp.content_length();
        if total.is_some_and(|t| t > MAX_DOWNLOAD_BYTES) {
            return Err(ApiError::Decode("download too large".into()));
        }
        let mut out = File::create(dest)?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| ApiError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > MAX_DOWNLOAD_BYTES {
                return Err(ApiError::Decode("download too large".into()));
            }
            out.write_all(&buf[..n])?;
            progress(done, total);
        }
        out.sync_all()?;
        Ok(())
    }
}

/// `a < b` for dotted versions such as `0.2.10`.
pub fn version_lt(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .take_while(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    parts(a) < parts(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_statuses_decode() {
        let p: LinkPoll = serde_json::from_str(r#"{"status":"pending"}"#).unwrap();
        assert!(matches!(p, LinkPoll::Pending));
        let p: LinkPoll = serde_json::from_str(
            r#"{"status":"approved","token":"ubl_x","token_id":3,"expires_at":"…","user":{"id":7,"username":"a","avatar_url":null}}"#,
        )
        .unwrap();
        assert!(
            matches!(p, LinkPoll::Approved { ref token, ref user } if token == "ubl_x" && user.id == 7)
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_lt("0.2.9", "0.2.10"));
        assert!(version_lt("0.1.0", "v1.0.0"));
        assert!(!version_lt("1.0.0", "1.0.0"));
    }

    #[test]
    fn urls_join_the_base() {
        let c = ApiClient::new("https://unbeaten.cc/", None);
        assert_eq!(c.url("/api/x"), "https://unbeaten.cc/api/x");
        assert_eq!(c.url("https://cdn.example/x"), "https://cdn.example/x");
    }
}

pub const RELEASES_PAGE: &str = "https://github.com/ofujiiro/UNBEATEN_APP/releases/latest";
const RELEASES_API: &str = "https://api.github.com/repos/ofujiiro/UNBEATEN_APP/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

/// A newer published release than this build, if GitHub says there is one.
pub fn check_for_update() -> Option<UpdateInfo> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let http = Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let release: Release = http
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    let version = release.tag_name.trim_start_matches('v').to_string();
    if release.draft || release.prerelease || !version_lt(CLIENT_VERSION, &version) {
        return None;
    }
    Some(UpdateInfo {
        version,
        url: release.html_url,
    })
}
