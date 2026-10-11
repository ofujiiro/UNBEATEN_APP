//! The game's `arcade-highscores.json`.
//!
//! Shape (one entry per song, difficulty and mode, keeping the best result):
//! `{"highScores": [{"song": "CUSTOM_<folder>/<Tier>\\<Mode>", "score", "accuracy",
//! "maxCombo", "notes": [{"timing", "count"}], "modifierMask", "level", "cleared",
//! "updateCount", …}]}`. Official songs have no `CUSTOM_` prefix.

use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::charts::sha256_hex;

/// One save entry. Unknown fields are kept so the entry is sent to the server
/// as the game wrote it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveEntry {
    pub song: String,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl SaveEntry {
    /// `(folder, tier)` for `CUSTOM_<folder>/<tier>\<mode>`; `None` for official songs.
    pub fn custom_key(&self) -> Option<(String, String)> {
        split_song_key(&self.song)
    }

    pub fn is_custom(&self) -> bool {
        self.custom_key().is_some()
    }

    /// Failed runs are saved with `cleared: false`. A missing flag counts as
    /// cleared, as on the server.
    pub fn cleared(&self) -> bool {
        self.rest
            .get("cleared")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    pub fn accuracy(&self) -> f64 {
        self.rest
            .get("accuracy")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    }

    pub fn score(&self) -> i64 {
        self.rest.get("score").and_then(Value::as_i64).unwrap_or(0)
    }

    pub fn max_combo(&self) -> i64 {
        self.rest
            .get("maxCombo")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    pub fn modifier_mask(&self) -> i64 {
        self.rest
            .get("modifierMask")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    pub fn miss_count(&self) -> i64 {
        self.rest
            .get("notes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|n| n.get("timing").and_then(Value::as_str) == Some("Miss"))
            .and_then(|n| n.get("count").and_then(Value::as_i64))
            .unwrap_or(0)
    }

    /// Changes whenever anything about the entry changes (a better score,
    /// another updateCount, …). serde_json's map is ordered by key, so the
    /// encoding is stable.
    pub fn fingerprint(&self) -> String {
        let value = serde_json::to_vec(self).unwrap_or_default();
        sha256_hex(&value)
    }
}

/// Same rule as UNBEATEN's `score_import.split_song_key`.
pub fn split_song_key(song: &str) -> Option<(String, String)> {
    let prefix = song.get(..7)?;
    if !prefix.eq_ignore_ascii_case("CUSTOM_") {
        return None;
    }
    let rest = &song[7..];
    let mut parts = rest.split(['/', '\\']);
    let folder = parts.next().unwrap_or("").trim().to_string();
    let diff = parts.next().unwrap_or("").trim().to_string();
    if folder.is_empty() {
        None
    } else {
        Some((folder, diff))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("can't read the save file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the save file isn't valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the save file has no highScores list")]
    Shape,
}

/// Parse the save. Accepts a UTF-8 BOM and, like the website, `highScores`,
/// `scores` or a bare list.
pub fn parse_save(data: &[u8]) -> Result<Vec<SaveEntry>, SaveError> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let value: Value = serde_json::from_slice(data)?;
    let list = match value {
        Value::Array(list) => list,
        Value::Object(mut obj) => match obj.remove("highScores").or_else(|| obj.remove("scores")) {
            Some(Value::Array(list)) => list,
            _ => return Err(SaveError::Shape),
        },
        _ => return Err(SaveError::Shape),
    };
    Ok(list
        .into_iter()
        .filter_map(|v| serde_json::from_value::<SaveEntry>(v).ok())
        .collect())
}

/// Read the save, retrying a few times if it doesn't parse: the game may be
/// in the middle of writing it.
pub fn read_save(path: &Path) -> Result<Vec<SaveEntry>, SaveError> {
    let mut last = None;
    for attempt in 0..5 {
        if attempt > 0 {
            sleep(Duration::from_millis(300));
        }
        match fs::read(path)
            .map_err(SaveError::from)
            .and_then(|d| parse_save(&d))
        {
            Ok(entries) => return Ok(entries),
            Err(SaveError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(SaveError::Io(e))
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(SaveError::Shape))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/arcade-highscores.json");

    #[test]
    fn parses_the_sample_save() {
        // Made-up data in the shape of a real arcade-highscores.json.
        let entries = parse_save(SAMPLE.as_bytes()).unwrap();
        assert_eq!(entries.len(), 11);
        let custom: Vec<_> = entries.iter().filter(|e| e.is_custom()).collect();
        assert_eq!(custom.len(), 8);
        assert_eq!(entries.iter().filter(|e| !e.cleared()).count(), 2);
        let graded = entries
            .iter()
            .find(|e| e.song == "CUSTOM_Graded/UNBEATABLE\\Classic")
            .unwrap();
        assert_eq!(
            graded.custom_key(),
            Some(("Graded".into(), "UNBEATABLE".into()))
        );
        assert_eq!((graded.max_combo(), graded.miss_count()), (372, 4));
        // Unknown fields survive a round trip.
        let back = serde_json::to_value(graded).unwrap();
        assert_eq!(back["grade"], 5);
        assert_eq!(back["notes"][0]["timing"], "Critical");
        let dt = entries
            .iter()
            .find(|e| e.song.ends_with("Beginner\\DoubleTime"))
            .unwrap();
        assert_eq!(dt.modifier_mask(), 20);
    }

    #[test]
    fn accepts_bom_and_other_shapes() {
        let mut bom = b"\xef\xbb\xbf".to_vec();
        bom.extend_from_slice(br#"{"scores":[{"song":"CUSTOM_A/Hard\\Classic"}]}"#);
        assert_eq!(parse_save(&bom).unwrap().len(), 1);
        assert_eq!(parse_save(br#"[{"song":"X"}]"#).unwrap().len(), 1);
        assert!(matches!(
            parse_save(br#"{"other":1}"#),
            Err(SaveError::Shape)
        ));
        assert!(matches!(
            parse_save(br#"{"highScores":[{"so"#),
            Err(SaveError::Json(_))
        ));
    }

    #[test]
    fn song_keys_split_like_the_server() {
        assert_eq!(
            split_song_key("CUSTOM_Flavor Foley - Static (Uluxite)/Star\\Classic"),
            Some(("Flavor Foley - Static (Uluxite)".into(), "Star".into()))
        );
        assert_eq!(
            split_song_key("custom_x/Hard\\Classic"),
            Some(("x".into(), "Hard".into()))
        );
        assert_eq!(split_song_key("FOREVER NOW/UNBEATABLE\\Classic"), None);
        assert_eq!(split_song_key("CUSTOM_/Hard"), None);
    }

    #[test]
    fn fingerprint_tracks_any_change() {
        let entries = parse_save(SAMPLE.as_bytes()).unwrap();
        let mut e = entries[0].clone();
        let before = e.fingerprint();
        assert_eq!(before, entries[0].clone().fingerprint());
        e.rest.insert("updateCount".into(), Value::from(9));
        assert_ne!(before, e.fingerprint());
    }

    #[test]
    fn read_retries_a_half_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arcade-highscores.json");
        fs::write(&path, br#"{"highScores":[{"song":"#).unwrap();
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                sleep(Duration::from_millis(350));
                fs::write(
                    &path,
                    br#"{"highScores":[{"song":"CUSTOM_A/Hard\\Classic"}]}"#,
                )
                .unwrap();
            })
        };
        let entries = read_save(&path).unwrap();
        writer.join().unwrap();
        assert_eq!(entries.len(), 1);
    }
}
