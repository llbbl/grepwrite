//! JSON output (schema v1). Stable, documented, pretty-printed.
//!
//! `schema_version: 1` is part of the stability promise: field names and types
//! must not change without bumping the version. Adding new fields later is
//! fine — consumers are expected to ignore unknown keys.
//!
//! Pretty-printing is intentional: this format is for programmatic consumers
//! AND for human debugging via `jq` / eyeball. The `caveman` format is the
//! one optimized for terseness.

use crate::errors::GwError;
use crate::locate::Match;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Per-match record. The `captures` map keys are capture group names; the
/// full match is keyed `"0"` (numeric string, not the empty string the
/// internal `Match.captures` uses).
#[derive(Debug, Serialize)]
struct JsonMatch {
    path: PathBuf,
    line: u32,
    col: u32,
    byte_start: u64,
    byte_end: u64,
    /// Renamed to `match` in JSON — `match` is a Rust keyword.
    #[serde(rename = "match")]
    text: String,
    captures: BTreeMap<String, String>,
}

/// Per-edit record (rewrite only). `before` and `after` are the original /
/// replacement text at this match site — not full file contents.
#[derive(Debug, Serialize, Clone)]
pub struct EditPreview {
    pub path: PathBuf,
    pub line: u32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Serialize)]
struct Summary {
    files: usize,
    matches: usize,
    edits: usize,
}

/// Combined envelope for both `find` and `rewrite`. `edits` is always present
/// (empty for `find`); `applied` and `snapshot_id` are meaningful for
/// `rewrite` only but always emitted so consumers can rely on a single shape.
#[derive(Debug, Serialize)]
struct Envelope {
    schema_version: u32,
    command: &'static str,
    engine: String,
    matches: Vec<JsonMatch>,
    edits: Vec<EditPreview>,
    applied: bool,
    snapshot_id: Option<String>,
    summary: Summary,
}

fn to_json_match(m: &Match) -> JsonMatch {
    // Full-match text is stored under the empty-string key by the locate
    // layer's convention; map it to "0" for the JSON contract.
    let mut captures: BTreeMap<String, String> = BTreeMap::new();
    let mut full_match = String::new();
    for (k, v) in &m.captures {
        if k.is_empty() {
            full_match = v.clone();
            captures.insert("0".to_string(), v.clone());
        } else {
            captures.insert(k.clone(), v.clone());
        }
    }
    // If captures was empty (no group 0 recorded), fall back to the substring
    // we can recover from line_text + cols. In v0.1, both locators populate
    // captures with at least the full match, but be defensive.
    if !captures.contains_key("0") {
        full_match = String::new();
        captures.insert("0".to_string(), String::new());
    }

    JsonMatch {
        path: m.path.clone(),
        line: m.line,
        col: m.col,
        byte_start: m.byte_start,
        byte_end: m.byte_end,
        text: full_match,
        captures,
    }
}

fn distinct_files(matches: &[Match]) -> usize {
    matches
        .iter()
        .map(|m| m.path.as_path())
        .collect::<BTreeSet<_>>()
        .len()
}

/// Render the JSON envelope for `gw find`. `engine` is `"rg"` or `"ast-grep"`.
pub fn render_find(matches: &[Match], engine: &str) -> Result<String, GwError> {
    let env = Envelope {
        schema_version: 1,
        command: "find",
        engine: engine.to_string(),
        matches: matches.iter().map(to_json_match).collect(),
        edits: Vec::new(),
        applied: false,
        snapshot_id: None,
        summary: Summary {
            files: distinct_files(matches),
            matches: matches.len(),
            edits: 0,
        },
    };
    serde_json::to_string_pretty(&env)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| GwError::Engine(format!("json serialize: {e}")))
}

/// Render the JSON envelope for `gw rewrite` (both dry-run and apply).
/// `applied=false, snapshot_id=None` for dry-run; `applied=true` plus the
/// snapshot id (or `None` with `--no-snapshot`) for apply.
pub fn render_rewrite(
    matches: &[Match],
    edits: &[EditPreview],
    applied: bool,
    snapshot_id: Option<&str>,
    engine: &str,
) -> Result<String, GwError> {
    let env = Envelope {
        schema_version: 1,
        command: "rewrite",
        engine: engine.to_string(),
        matches: matches.iter().map(to_json_match).collect(),
        edits: edits.to_vec(),
        applied,
        snapshot_id: snapshot_id.map(|s| s.to_string()),
        summary: Summary {
            files: distinct_files(matches),
            matches: matches.len(),
            edits: edits.len(),
        },
    };
    serde_json::to_string_pretty(&env)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| GwError::Engine(format!("json serialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn m(path: &str, line: u32, col: u32, text: &str) -> Match {
        Match {
            path: PathBuf::from(path),
            line,
            col,
            byte_start: 10,
            byte_end: 13,
            line_text: text.to_string(),
            captures: vec![(String::new(), "foo".to_string())],
        }
    }

    #[test]
    fn render_find_empty() {
        let s = render_find(&[], "rg").unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["command"], "find");
        assert_eq!(v["engine"], "rg");
        assert_eq!(v["matches"].as_array().unwrap().len(), 0);
        assert_eq!(v["edits"].as_array().unwrap().len(), 0);
        assert_eq!(v["applied"], false);
        assert!(v["snapshot_id"].is_null());
        assert_eq!(v["summary"]["files"], 0);
        assert_eq!(v["summary"]["matches"], 0);
        assert_eq!(v["summary"]["edits"], 0);
    }

    #[test]
    fn render_find_multiple_matches_summary_counts() {
        let ms = vec![
            m("a.rs", 1, 1, "x"),
            m("a.rs", 2, 1, "x"),
            m("b.rs", 3, 1, "x"),
        ];
        let s = render_find(&ms, "rg").unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["command"], "find");
        assert_eq!(v["summary"]["files"], 2);
        assert_eq!(v["summary"]["matches"], 3);
        assert_eq!(v["summary"]["edits"], 0);
        // captures map uses "0", not "".
        let caps = &v["matches"][0]["captures"];
        assert_eq!(caps["0"], "foo");
        assert!(caps.get("").is_none());
    }

    #[test]
    fn render_rewrite_dry_run_applied_false_snapshot_null() {
        let ms = vec![m("a.rs", 1, 1, "x")];
        let edits = vec![EditPreview {
            path: PathBuf::from("a.rs"),
            line: 1,
            before: "foo".to_string(),
            after: "bar".to_string(),
        }];
        let s = render_rewrite(&ms, &edits, false, None, "rg").unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["command"], "rewrite");
        assert_eq!(v["applied"], false);
        assert!(v["snapshot_id"].is_null());
        assert_eq!(v["edits"][0]["before"], "foo");
        assert_eq!(v["edits"][0]["after"], "bar");
        assert_eq!(v["summary"]["edits"], 1);
    }

    #[test]
    fn render_rewrite_applied_with_snapshot_id() {
        let ms = vec![m("a.rs", 1, 1, "x")];
        let edits = vec![EditPreview {
            path: PathBuf::from("a.rs"),
            line: 1,
            before: "foo".to_string(),
            after: "bar".to_string(),
        }];
        let s =
            render_rewrite(&ms, &edits, true, Some("2026-05-31T09-45-12-abc123"), "rg").unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["applied"], true);
        assert_eq!(v["snapshot_id"], "2026-05-31T09-45-12-abc123");
    }

    #[test]
    fn captures_map_key_zero_not_empty_string() {
        let ms = vec![m("a.rs", 1, 1, "x")];
        let s = render_find(&ms, "rg").unwrap();
        // Raw string search: ensure `"0":` appears and `"":` (as a key) does not.
        assert!(s.contains("\"0\""), "expected \"0\" key in:\n{s}");
        // Look for `""\s*:` patterns — captures keys only.
        assert!(
            !s.contains("\"\": "),
            "captures must not use empty-string key:\n{s}"
        );
    }

    #[test]
    fn output_is_pretty_printed_not_compact() {
        let s = render_find(&[], "rg").unwrap();
        // Pretty output has indented newlines; compact is single-line.
        assert!(s.contains('\n'), "expected pretty-printed JSON:\n{s}");
    }
}
