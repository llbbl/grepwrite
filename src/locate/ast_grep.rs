//! ast-grep-backed locator, used when `Query::in_scope` is set.
//!
//! Spawns `ast-grep scan --inline-rules <JSON> --json=stream` and parses the
//! line-delimited JSON output (one match object per line).
//!
//! ## Why `scan` and not `run`?
//!
//! ast-grep 0.43 splits CLI commands: `run` accepts a single `--pattern` but
//! has no way to express a composite rule like "pattern X inside node kind
//! Y". `scan` accepts `--inline-rules` with a full YAML/JSON rule object,
//! which is the simplest correct way to encode `--in <scope>` without
//! writing a temp file. The `--json=stream` mode is identical between the
//! two commands.
//!
//! ## Observed JSON schema (ast-grep 0.43, `--json=stream`)
//!
//! Each non-empty line is a self-contained JSON object:
//!
//! ```json
//! {
//!   "text": "TODO",
//!   "range": {
//!     "byteOffset": {"start": 19, "end": 23},
//!     "start": {"line": 1, "column": 2},
//!     "end":   {"line": 1, "column": 6}
//!   },
//!   "file": "sample.ts",
//!   "lines": "  TODO();",
//!   "language": "TypeScript",
//!   "ruleId": "gw",
//!   ...
//! }
//! ```
//!
//! - `range.byteOffset.{start,end}` — **file-relative** byte offsets. This is
//!   different from [`crate::locate::rg::RgLocator`] (which emits
//!   **line-relative** offsets), and is the asymmetry called out on the
//!   [`Match`] doc-comment. The mutate layer currently re-derives file
//!   offsets from `line + line_relative`; passing it file-relative offsets
//!   directly would also work but isn't required for v0.1.
//! - `range.start.{line,column}` — **0-indexed**. We add 1 when populating
//!   [`Match::line`] and [`Match::col`] (which are 1-indexed by trait
//!   contract).
//! - `text` — the matched node's source text. Stored as the only entry in
//!   [`Match::captures`] under the empty-string name (same convention as
//!   `RgLocator`). ast-grep also emits `metaVariables` for `$X`-style
//!   captures; we ignore those for v0.1 — the mutate template layer
//!   re-extracts captures with the `regex` crate for symmetry across
//!   engines.
//!
//! ## Scope translation
//!
//! `--in <scope>` is mapped to a tree-sitter node `kind` via the static
//! [`SCOPE_KIND`] table. The set is intentionally narrow for v0.1
//! (TypeScript / JavaScript / Python / Rust); broadening it is a contained
//! change because the locate engine is hidden behind the [`Locate`] trait.
//!
//! ## Exit code mapping
//!
//! `ast-grep scan` exits 0 on both "matches found" and "no matches". A
//! non-zero exit means a real CLI / rule error, which we surface as
//! [`GwError::Engine`] with stderr included.

use crate::errors::GwError;
use crate::locate::{Locate, Match, Query};

use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct AstGrepLocator;

/// `(scope, language, tree-sitter node kind)` table.
///
/// `language` is the long-form ast-grep language name (e.g. `typescript`,
/// not the `-t ts` shorthand). [`type_filter_to_language`] handles the
/// translation from the user-facing `--type` value.
///
/// Missing combinations (e.g. `class` for `rust`) are an error, not a
/// silent no-op — a query that can never match is a user mistake worth
/// surfacing early.
const SCOPE_KIND: &[(&str, &str, &str)] = &[
    ("function", "typescript", "function_declaration"),
    ("function", "javascript", "function_declaration"),
    ("function", "python", "function_definition"),
    ("function", "rust", "function_item"),
    ("class", "typescript", "class_declaration"),
    ("class", "javascript", "class_declaration"),
    ("class", "python", "class_definition"),
    // `imports`: pick the common single-import node. Python also has
    // `import_from_statement`; v0.1 covers only plain `import` — broaden
    // here if users hit the gap.
    ("imports", "typescript", "import_statement"),
    ("imports", "javascript", "import_statement"),
    ("imports", "python", "import_statement"),
    ("imports", "rust", "use_declaration"),
    // `comments`: tree-sitter-rust splits `line_comment` vs `block_comment`;
    // v0.1 covers line comments only.
    ("comments", "typescript", "comment"),
    ("comments", "javascript", "comment"),
    ("comments", "python", "comment"),
    ("comments", "rust", "line_comment"),
];

/// Translate the user-facing `--type` value (`ts`, `js`, `py`, `rs`, ...) to
/// the ast-grep long-form language name used in the rule. Returns `None` if
/// the value isn't one we support for `--in <scope>`.
fn type_filter_to_language(t: &str) -> Option<&'static str> {
    match t {
        "ts" | "typescript" => Some("typescript"),
        "js" | "javascript" => Some("javascript"),
        "py" | "python" => Some("python"),
        "rs" | "rust" => Some("rust"),
        _ => None,
    }
}

/// Look up the tree-sitter node kind that `--in <scope>` translates to for a
/// given language. Returns a clear [`GwError::Engine`] when the
/// `(scope, language)` pair is not in [`SCOPE_KIND`].
fn scope_to_kind(scope: &str, language: &str) -> Result<&'static str, GwError> {
    SCOPE_KIND
        .iter()
        .find(|(s, l, _)| *s == scope && *l == language)
        .map(|(_, _, k)| *k)
        .ok_or_else(|| {
            GwError::Engine(format!(
                "--in {scope} is not supported for language '{language}' in v0.1"
            ))
        })
}

impl Locate for AstGrepLocator {
    fn run(&self, query: &Query) -> Result<Vec<Match>, GwError> {
        let scope = query.in_scope.as_deref().ok_or_else(|| {
            GwError::Engine(
                "AstGrepLocator requires --in <scope>; use RgLocator otherwise".to_string(),
            )
        })?;

        let type_filter = query.type_filter.as_deref().ok_or_else(|| {
            GwError::Engine(
                "--in <scope> requires --type to pick a language (e.g. -t ts)".to_string(),
            )
        })?;

        let language = type_filter_to_language(type_filter).ok_or_else(|| {
            GwError::Engine(format!(
                "--in <scope> not supported for --type '{type_filter}' in v0.1 \
                 (supported: ts, js, py, rs)"
            ))
        })?;

        let kind = scope_to_kind(scope, language)?;

        // Compose the inline rule. `pattern` is the user's ast-grep pattern;
        // `inside` constrains matches to descend from a node of the chosen
        // kind. `stopBy: end` tells ast-grep to walk all ancestors, not just
        // the immediate parent — without it, `pattern: TODO` inside a
        // nested expression wouldn't match because the immediate parent
        // isn't the function node.
        let rule = serde_json::json!({
            "id": "gw",
            "language": language,
            "rule": {
                "pattern": query.pattern,
                "inside": { "kind": kind, "stopBy": "end" }
            }
        });
        let rule_str =
            serde_json::to_string(&rule).map_err(|e| GwError::Engine(format!("rule json: {e}")))?;

        let mut cmd = Command::new("ast-grep");
        cmd.arg("scan")
            .arg("--inline-rules")
            .arg(&rule_str)
            .arg("--json=stream");

        for g in &query.globs {
            cmd.arg("--globs").arg(g);
        }

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

        let mut child = cmd
            .spawn()
            .map_err(|e| GwError::Engine(format!("failed to spawn ast-grep: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GwError::Engine("failed to capture ast-grep stdout".into()))?;
        let matches = parse_ast_grep_events(BufReader::new(stdout))?;

        let output = child
            .wait_with_output()
            .map_err(|e| GwError::Engine(format!("ast-grep wait failed: {e}")))?;

        match output.status.code() {
            Some(0) => Ok(matches),
            Some(code) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(GwError::Engine(format!(
                    "ast-grep failed (exit {code}): {}",
                    stderr.trim()
                )))
            }
            None => Err(GwError::Engine("ast-grep terminated by signal".into())),
        }
    }
}

// ----------------------------------------------------------------------------
// JSON parsing
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SgEvent {
    file: String,
    text: String,
    lines: Option<String>,
    range: SgRange,
}

#[derive(Debug, Deserialize)]
struct SgRange {
    #[serde(rename = "byteOffset")]
    byte_offset: SgByteOffset,
    start: SgPos,
}

#[derive(Debug, Deserialize)]
struct SgByteOffset {
    start: u64,
    end: u64,
}

#[derive(Debug, Deserialize)]
struct SgPos {
    line: u32,
    column: u32,
}

/// Parse ast-grep's `--json=stream` output into a vector of [`Match`].
///
/// Each non-empty line is expected to be a self-contained JSON object.
/// Malformed lines are skipped silently (same justification as
/// `RgLocator::parse_rg_events` — ast-grep may emit non-JSON warnings to
/// stdout in some edge cases, and we'd rather drop one event than fail a
/// whole query).
pub fn parse_ast_grep_events<R: BufRead>(reader: R) -> Result<Vec<Match>, GwError> {
    let mut out = Vec::new();
    for line in reader.lines() {
        let line =
            line.map_err(|e| GwError::Engine(format!("ast-grep stdout read failed: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let event: SgEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let line_text = event
            .lines
            .unwrap_or_default()
            .trim_end_matches('\n')
            .to_string();

        out.push(Match {
            path: PathBuf::from(event.file),
            // ast-grep uses 0-indexed line/column; our trait contract is 1-indexed.
            line: event.range.start.line.saturating_add(1),
            col: event.range.start.column.saturating_add(1),
            // File-relative byte offsets (NOT line-relative) — see module doc.
            byte_start: event.range.byte_offset.start,
            byte_end: event.range.byte_offset.end,
            line_text,
            captures: vec![(String::new(), event.text)],
        });
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SINGLE: &str = r#"
{"text":"TODO","range":{"byteOffset":{"start":19,"end":23},"start":{"line":1,"column":2},"end":{"line":1,"column":6}},"file":"sample.ts","lines":"  TODO();","language":"TypeScript"}
"#;

    const MULTI: &str = r#"
{"text":"TODO","range":{"byteOffset":{"start":19,"end":23},"start":{"line":1,"column":2},"end":{"line":1,"column":6}},"file":"a.ts","lines":"  TODO();","language":"TypeScript"}
{"text":"TODO","range":{"byteOffset":{"start":50,"end":54},"start":{"line":4,"column":0},"end":{"line":4,"column":4}},"file":"b.ts","lines":"TODO();","language":"TypeScript"}
"#;

    const WITH_MALFORMED: &str = r#"
not json at all
{"text":"X","range":{"byteOffset":{"start":0,"end":1},"start":{"line":0,"column":0},"end":{"line":0,"column":1}},"file":"x.ts","lines":"X"}
{"missing":"required fields"}
"#;

    fn parse(s: &str) -> Vec<Match> {
        parse_ast_grep_events(Cursor::new(s.trim_start())).expect("parse ok")
    }

    #[test]
    fn parses_single_event() {
        let ms = parse(SINGLE);
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.path, PathBuf::from("sample.ts"));
        assert_eq!(m.line, 2, "ast-grep line is 0-indexed; we add 1");
        assert_eq!(m.col, 3, "ast-grep column is 0-indexed; we add 1");
        assert_eq!(m.byte_start, 19);
        assert_eq!(m.byte_end, 23);
        assert_eq!(m.line_text, "  TODO();");
        assert_eq!(m.captures, vec![(String::new(), "TODO".to_string())]);
    }

    #[test]
    fn parses_multiple_events() {
        let ms = parse(MULTI);
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].path, PathBuf::from("a.ts"));
        assert_eq!(ms[1].path, PathBuf::from("b.ts"));
        assert_eq!(ms[1].byte_start, 50);
        assert_eq!(ms[1].col, 1);
    }

    #[test]
    fn skips_malformed_lines() {
        let ms = parse(WITH_MALFORMED);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].path, PathBuf::from("x.ts"));
    }

    #[test]
    fn scope_to_kind_known_pairs() {
        assert_eq!(
            scope_to_kind("function", "typescript").unwrap(),
            "function_declaration"
        );
        assert_eq!(scope_to_kind("function", "rust").unwrap(), "function_item");
        assert_eq!(
            scope_to_kind("class", "python").unwrap(),
            "class_definition"
        );
        assert_eq!(scope_to_kind("imports", "rust").unwrap(), "use_declaration");
        assert_eq!(scope_to_kind("comments", "rust").unwrap(), "line_comment");
    }

    #[test]
    fn scope_to_kind_rejects_class_in_rust() {
        let err = scope_to_kind("class", "rust").unwrap_err();
        assert!(matches!(err, GwError::Engine(_)));
    }

    #[test]
    fn scope_to_kind_rejects_unknown_language() {
        let err = scope_to_kind("function", "ocaml").unwrap_err();
        assert!(matches!(err, GwError::Engine(_)));
    }

    #[test]
    fn type_filter_translates_short_forms() {
        assert_eq!(type_filter_to_language("ts"), Some("typescript"));
        assert_eq!(type_filter_to_language("typescript"), Some("typescript"));
        assert_eq!(type_filter_to_language("py"), Some("python"));
        assert_eq!(type_filter_to_language("rs"), Some("rust"));
        assert_eq!(type_filter_to_language("go"), None);
    }

    #[test]
    fn run_requires_in_scope() {
        let q = Query {
            pattern: "TODO".into(),
            type_filter: Some("ts".into()),
            ..Query::default()
        };
        let err = AstGrepLocator.run(&q).unwrap_err();
        match err {
            GwError::Engine(msg) => assert!(msg.contains("requires --in")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn run_requires_type_filter() {
        let q = Query {
            pattern: "TODO".into(),
            in_scope: Some("function".into()),
            type_filter: None,
            ..Query::default()
        };
        let err = AstGrepLocator.run(&q).unwrap_err();
        match err {
            GwError::Engine(msg) => assert!(msg.contains("--type")),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
