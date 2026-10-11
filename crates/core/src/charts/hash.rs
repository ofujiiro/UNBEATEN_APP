//! The `ubh1` chart hash, identical to UNBEATEN's `map_engine.chart_hash`.
//!
//! Both implementations are tested against `tests/fixtures/chart_hash_vectors.json`
//! (a copy of `backend/tests/fixtures/chart_hash_vectors.json` in UNBEATEN), so
//! change neither without the other.
//!
//! Works on raw bytes: strip a UTF-8 BOM, split on `\n`, trim ASCII whitespace,
//! keep the lines of `[Difficulty]`, `[TimingPoints]` and `[HitObjects]` (no blank
//! lines, no `//` comments), sort the `[HitObjects]` lines byte-wise, then
//! `sha256("ubh1\n" + header + "\n" + line + "\n" …)`. No hit objects → `None`.

use sha2::{Digest, Sha256};

pub const CHART_HASH_ALGO: &str = "ubh1";

const SECTIONS: [&[u8]; 3] = [b"[Difficulty]", b"[TimingPoints]", b"[HitObjects]"];
const HIT_OBJECTS: usize = 2;

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

fn trim(mut line: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = line {
        if !is_ws(*first) {
            break;
        }
        line = rest;
    }
    while let [rest @ .., last] = line {
        if !is_ws(*last) {
            break;
        }
        line = rest;
    }
    line
}

/// Hex sha256 of the chart's gameplay sections, or `None` if it has no hit objects.
pub fn chart_hash(data: &[u8]) -> Option<String> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let mut kept: [Vec<&[u8]>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut current: Option<usize> = None;
    for raw in data.split(|&b| b == b'\n') {
        let line = trim(raw);
        if line.is_empty() {
            continue;
        }
        if line[0] == b'[' {
            current = SECTIONS.iter().position(|s| *s == line);
            continue;
        }
        if line.starts_with(b"//") {
            continue;
        }
        if let Some(i) = current {
            kept[i].push(line);
        }
    }
    if kept[HIT_OBJECTS].is_empty() {
        return None;
    }
    kept[HIT_OBJECTS].sort_unstable();

    let mut h = Sha256::new();
    h.update(CHART_HASH_ALGO.as_bytes());
    h.update(b"\n");
    for (name, lines) in SECTIONS.iter().zip(kept.iter()) {
        h.update(name);
        h.update(b"\n");
        for line in lines {
            h.update(line);
            h.update(b"\n");
        }
    }
    Some(hex(&h.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Hex sha256 of arbitrary bytes.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        input_b64: String,
        expected: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        algo: String,
        cases: Vec<Case>,
        same_as_base: Vec<String>,
        null: Vec<String>,
    }

    fn vectors() -> Vectors {
        let raw = include_str!("../../tests/fixtures/chart_hash_vectors.json");
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn matches_every_shared_vector() {
        let v = vectors();
        assert_eq!(v.algo, CHART_HASH_ALGO);
        for case in &v.cases {
            let input = base64::engine::general_purpose::STANDARD
                .decode(&case.input_b64)
                .unwrap();
            assert_eq!(chart_hash(&input), case.expected, "case {}", case.name);
        }
        let base = v.cases.iter().find(|c| c.name == "base").unwrap();
        for name in &v.same_as_base {
            let c = v.cases.iter().find(|c| &c.name == name).unwrap();
            assert_eq!(c.expected, base.expected, "{name}");
        }
        for name in &v.null {
            let c = v.cases.iter().find(|c| &c.name == name).unwrap();
            assert!(c.expected.is_none(), "{name}");
        }
    }

    #[test]
    fn trims_only_ascii_whitespace() {
        assert_eq!(trim(b" \t a b \r\n"), b"a b");
        assert_eq!(trim(b"\x0b x"), b"\x0b x");
    }
}
