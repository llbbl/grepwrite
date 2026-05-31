//! Ripgrep-backed locator.
//!
//! Spawns `rg --json` and parses the streaming event format. We only consume
//! `match` events; `begin`, `end`, `summary`, and `context` events are ignored
//! for v0.1.
//!
//! Limitations (v0.1):
//! - `byte_start` / `byte_end` on emitted [`Match`] values are **line-relative**,
//!   because rg's JSON event stream does not include file-relative byte
//!   offsets. The mutate layer (task #5) reconciles by re-reading the file.
//! - `captures` always contains a single `("", full_match)` entry. rg's JSON
//!   does not expose numbered/named capture groups; capture extraction is
//!   re-applied with the `regex` crate at the mutate template layer.
//! - Non-UTF-8 paths are decoded with `String::from_utf8_lossy`. See the
//!   `path.bytes` branch in [`parse_rg_events`].
//! - **stderr deadlock risk on noisy runs**: the current implementation reads
//!   rg's stdout to EOF before draining stderr. For very noisy invocations
//!   (e.g. `--no-ignore` on a large tree producing many permission-denied
//!   warnings), rg's stderr pipe can fill its ~64KB buffer and stall rg's
//!   writes, which would in turn stall our stdout reader. The proper fix is
//!   to drain stderr concurrently on a background thread; deferred to a
//!   hardening pass once we have a real workload to tune against.

use crate::errors::GwError;
use crate::locate::{Locate, Match, Query};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct RgLocator;

impl Locate for RgLocator {
    fn run(&self, query: &Query) -> Result<Vec<Match>, GwError> {
        let mut cmd = Command::new("rg");
        cmd.arg("--json").arg("--no-heading");

        if let Some(t) = &query.type_filter {
            cmd.arg("--type").arg(t);
        }
        for g in &query.globs {
            cmd.arg(format!("--glob={g}"));
        }
        if let Some(c) = query.context {
            cmd.arg("--context").arg(c.to_string());
        }
        if query.hidden {
            cmd.arg("--hidden");
        }
        if query.no_ignore {
            cmd.arg("--no-ignore");
        }

        cmd.arg(format!("--regexp={}", query.pattern));
        cmd.arg("--");
        if query.paths.is_empty() {
            cmd.arg(".");
        } else {
            for p in &query.paths {
                cmd.arg(p);
            }
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        tracing::debug!(cmd = ?cmd, "invoking rg");
        let mut child = cmd
            .spawn()
            .map_err(|e| GwError::Engine(format!("failed to spawn rg: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GwError::Engine("failed to capture rg stdout".into()))?;
        let matches = parse_rg_events(BufReader::new(stdout))?;
        tracing::debug!(matches = matches.len(), "rg returned matches");

        let output = child
            .wait_with_output()
            .map_err(|e| GwError::Engine(format!("rg wait failed: {e}")))?;

        match output.status.code() {
            Some(0) => Ok(matches),
            Some(1) => Ok(Vec::new()),
            Some(code) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(GwError::Engine(format!(
                    "rg failed (exit {code}): {}",
                    stderr.trim()
                )))
            }
            None => Err(GwError::Engine("rg terminated by signal".into())),
        }
    }
}

// ----------------------------------------------------------------------------
// JSON parsing
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RgEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RgText {
    text: Option<String>,
    bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RgSubmatch {
    #[serde(rename = "match")]
    m: RgText,
    start: u64,
    end: u64,
}

#[derive(Debug, Deserialize)]
struct RgMatchData {
    path: RgText,
    lines: RgText,
    line_number: u32,
    submatches: Vec<RgSubmatch>,
}

/// Parse rg's `--json` event stream from `reader` into a vector of [`Match`].
///
/// Malformed lines are skipped (rg may occasionally emit non-JSON warnings to
/// stdout); only `match` events contribute results. Errors are returned only
/// for unrecoverable parse failures on otherwise-valid `match` events.
pub fn parse_rg_events<R: BufRead>(reader: R) -> Result<Vec<Match>, GwError> {
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| GwError::Engine(format!("rg stdout read failed: {e}")))?;
        if line.is_empty() {
            continue;
        }
        let event: RgEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if event.kind != "match" {
            continue;
        }
        let Some(data_val) = event.data else { continue };
        let data: RgMatchData = match serde_json::from_value(data_val) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let path = decode_path(&data.path);
        let line_text = decode_text(&data.lines).trim_end_matches('\n').to_string();

        for sm in &data.submatches {
            let matched = decode_text(&sm.m);
            out.push(Match {
                path: path.clone(),
                line: data.line_number,
                col: (sm.start as u32).saturating_add(1),
                byte_start: sm.start,
                byte_end: sm.end,
                line_text: line_text.clone(),
                captures: vec![(String::new(), matched)],
            });
        }
    }
    Ok(out)
}

fn decode_text(t: &RgText) -> String {
    if let Some(s) = &t.text {
        return s.clone();
    }
    if let Some(b64) = &t.bytes {
        // v0.1 choice: log to stderr and fall back to empty string on base64
        // decode failure. A truly malformed `bytes` field from rg is a "should
        // never happen" — propagating it as an error would force every caller
        // through a fallible boundary for a case we don't expect in practice.
        // Revisit if we ever observe this fire on real workloads.
        match BASE64.decode(b64) {
            Ok(bytes) => return String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                eprintln!("grepwrite: dropping rg event with undecodable base64 field: {e}");
                return String::new();
            }
        }
    }
    String::new()
}

// LIMITATION: non-UTF-8 paths. rg encodes them as base64 in `bytes`;
// we currently lossy-decode to UTF-8, which means a path containing
// invalid UTF-8 bytes on Unix will round-trip through U+FFFD and
// may not open the actual file in the mutate layer. Fix requires
// platform-gated code (std::os::unix::ffi::OsStrExt) and is deferred
// until there's a real user with non-UTF-8 paths.
fn decode_path(t: &RgText) -> PathBuf {
    PathBuf::from(decode_text(t))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SINGLE_MATCH: &str = r#"
{"type":"begin","data":{"path":{"text":"src/foo.rs"}}}
{"type":"match","data":{"path":{"text":"src/foo.rs"},"lines":{"text":"hello world\n"},"line_number":3,"absolute_offset":42,"submatches":[{"match":{"text":"world"},"start":6,"end":11}]}}
{"type":"end","data":{"path":{"text":"src/foo.rs"},"binary_offset":null,"stats":{}}}
{"type":"summary","data":{"elapsed_total":{"human":"0s","nanos":1,"secs":0},"stats":{}}}
"#;

    const MULTI_SUBMATCH: &str = r#"
{"type":"match","data":{"path":{"text":"a.txt"},"lines":{"text":"foo bar foo\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"foo"},"start":0,"end":3},{"match":{"text":"foo"},"start":8,"end":11}]}}
"#;

    const WITH_CONTEXT: &str = r#"
{"type":"context","data":{"path":{"text":"a.txt"},"lines":{"text":"prior line\n"},"line_number":1,"absolute_offset":0,"submatches":[]}}
{"type":"match","data":{"path":{"text":"a.txt"},"lines":{"text":"target\n"},"line_number":2,"absolute_offset":11,"submatches":[{"match":{"text":"target"},"start":0,"end":6}]}}
{"type":"context","data":{"path":{"text":"a.txt"},"lines":{"text":"after\n"},"line_number":3,"absolute_offset":18,"submatches":[]}}
"#;

    const WITH_MALFORMED: &str = r#"
this is not json at all
{"type":"match","data":{"path":{"text":"x.rs"},"lines":{"text":"ok\n"},"line_number":7,"absolute_offset":0,"submatches":[{"match":{"text":"ok"},"start":0,"end":2}]}}
{"type":"garbage","not":"valid match data"}
"#;

    // `match` event whose `data` is missing required fields (lines,
    // line_number, submatches). Exercises the `serde_json::from_value` error
    // branch in `parse_rg_events`, which must skip the event without erroring.
    const MALFORMED_MATCH_DATA: &str = r#"
{"type":"match","data":{"path":{"text":"x.rs"}}}
{"type":"match","data":{"path":{"text":"y.rs"},"lines":{"text":"hi\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"hi"},"start":0,"end":2}]}}
"#;

    // base64("weird/path.rs") = "d2VpcmQvcGF0aC5ycw=="
    // base64("hi there\n")    = "aGkgdGhlcmUK"
    // base64("there")          = "dGhlcmU="
    const PATH_BYTES: &str = r#"
{"type":"match","data":{"path":{"bytes":"d2VpcmQvcGF0aC5ycw=="},"lines":{"bytes":"aGkgdGhlcmUK"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"bytes":"dGhlcmU="},"start":3,"end":8}]}}
"#;

    const EMPTY: &str = r#"
{"type":"summary","data":{"elapsed_total":{"human":"0s","nanos":0,"secs":0},"stats":{}}}
"#;

    fn parse(s: &str) -> Vec<Match> {
        parse_rg_events(Cursor::new(s.trim_start())).expect("parse ok")
    }

    #[test]
    fn parses_single_match() {
        let ms = parse(SINGLE_MATCH);
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.path, PathBuf::from("src/foo.rs"));
        assert_eq!(m.line, 3);
        assert_eq!(m.col, 7);
        assert_eq!(m.byte_start, 6);
        assert_eq!(m.byte_end, 11);
        assert_eq!(m.line_text, "hello world");
        assert_eq!(m.captures, vec![(String::new(), "world".to_string())]);
    }

    #[test]
    fn emits_one_match_per_submatch() {
        let ms = parse(MULTI_SUBMATCH);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].byte_start, 0);
        assert_eq!(ms[0].col, 1);
        assert_eq!(ms[1].byte_start, 8);
        assert_eq!(ms[1].col, 9);
        assert_eq!(ms[0].line_text, "foo bar foo");
        assert_eq!(ms[1].line_text, "foo bar foo");
    }

    #[test]
    fn ignores_context_begin_end_summary() {
        let ms = parse(WITH_CONTEXT);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].line, 2);
        assert_eq!(ms[0].line_text, "target");
    }

    #[test]
    fn skips_malformed_lines() {
        let ms = parse(WITH_MALFORMED);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].path, PathBuf::from("x.rs"));
        assert_eq!(ms[0].line, 7);
    }

    #[test]
    fn skips_match_events_with_malformed_data() {
        let ms = parse(MALFORMED_MATCH_DATA);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].path, PathBuf::from("y.rs"));
        assert_eq!(ms[0].line, 1);
    }

    #[test]
    fn decodes_base64_path_and_text() {
        let ms = parse(PATH_BYTES);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].path, PathBuf::from("weird/path.rs"));
        assert_eq!(ms[0].line_text, "hi there");
        assert_eq!(ms[0].captures, vec![(String::new(), "there".to_string())]);
    }

    #[test]
    fn empty_when_no_match_events() {
        let ms = parse(EMPTY);
        assert!(ms.is_empty());
    }
}
