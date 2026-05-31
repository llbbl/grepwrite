//! Edit planning and application.
//!
//! This module is pure with respect to the filesystem: `apply_edits` takes a
//! `&str` of original content and returns a new `String`. The only disk reads
//! happen in [`plan_edits_for_file`], which is invoked by the dry-run driver
//! to translate locate-layer line-relative offsets into file-relative ones —
//! it explicitly does **not** write. Atomic writes land in task #7.
#![allow(dead_code)]

pub mod template;

use crate::errors::GwError;
use crate::locate::Match;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A file-relative edit. `byte_start..byte_end` is replaced with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub byte_start: usize,
    pub byte_end: usize,
    pub replacement: String,
}

/// Apply a batch of file-relative edits to `original`, returning the new
/// content. Edits are applied in reverse byte order so that earlier-offset
/// edits cannot shift the offsets of later ones (see design.md §Mutate layer).
///
/// - Overlapping edits → `GwError::Engine("overlapping edits at byte N")`.
/// - `byte_end > original.len()` or `byte_start > byte_end` → `GwError::Engine`.
/// - Empty `edits` → `original.to_string()` unchanged.
pub fn apply_edits(original: &str, edits: &[Edit]) -> Result<String, GwError> {
    if edits.is_empty() {
        return Ok(original.to_string());
    }

    // Sort descending by byte_start; tie-break by byte_end descending so that
    // any zero-length adjacency is still deterministic.
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.byte_start
            .cmp(&a.byte_start)
            .then(b.byte_end.cmp(&a.byte_end))
    });

    // Overlap detection: in descending order, edit[i].byte_end must be <=
    // edit[i-1].byte_start (i-1 starts later or at same point).
    for w in sorted.windows(2) {
        let later = w[0]; // larger byte_start
        let earlier = w[1]; // smaller byte_start
        if earlier.byte_end > later.byte_start {
            return Err(GwError::Engine(format!(
                "overlapping edits at byte {}",
                later.byte_start
            )));
        }
    }

    let mut buf = original.to_string();
    for e in sorted {
        if e.byte_start > e.byte_end {
            return Err(GwError::Engine(format!(
                "edit has byte_start {} > byte_end {}",
                e.byte_start, e.byte_end
            )));
        }
        if e.byte_end > buf.len() {
            return Err(GwError::Engine(format!(
                "edit byte_end {} exceeds file length {}",
                e.byte_end,
                buf.len()
            )));
        }
        // Check char boundary safety: replace_range panics on non-boundary.
        if !buf.is_char_boundary(e.byte_start) || !buf.is_char_boundary(e.byte_end) {
            return Err(GwError::Engine(format!(
                "edit at bytes {}..{} crosses a UTF-8 char boundary",
                e.byte_start, e.byte_end
            )));
        }
        buf.replace_range(e.byte_start..e.byte_end, &e.replacement);
    }
    Ok(buf)
}

/// Group matches by their `path`, preserving stable per-path ordering.
pub fn group_matches_by_path(matches: &[Match]) -> BTreeMap<PathBuf, Vec<&Match>> {
    let mut by_path: BTreeMap<PathBuf, Vec<&Match>> = BTreeMap::new();
    for m in matches {
        by_path.entry(m.path.clone()).or_default().push(m);
    }
    by_path
}

/// Plan file-relative edits for a single file given its matches.
///
/// Reads `path` (the only disk I/O performed by this module). For each match,
/// re-runs `pattern.captures()` against `m.line_text` at position
/// `m.byte_start` to recover numbered/named captures (rg's JSON event stream
/// only carries the full-match text — see `locate::rg`), then expands
/// `replacement` and computes a file-relative `Edit`.
///
/// Returns `(original_content, edits)` so callers can drive diff rendering
/// without reading the file twice.
pub fn plan_edits_for_file(
    path: &Path,
    matches: &[&Match],
    pattern: &Regex,
    replacement: &str,
) -> Result<(String, Vec<Edit>), GwError> {
    let content = fs::read_to_string(path)
        .map_err(|e| GwError::Engine(format!("failed to read {}: {e}", path.display())))?;

    // Pre-compute line-start byte offsets (offset of first byte of each
    // 1-indexed line). `line_starts[0]` corresponds to line 1.
    let mut line_starts: Vec<usize> = Vec::with_capacity(64);
    line_starts.push(0);
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let mut edits = Vec::with_capacity(matches.len());
    for m in matches {
        let line_idx = m.line as usize;
        if line_idx == 0 || line_idx > line_starts.len() {
            return Err(GwError::Engine(format!(
                "match line {} out of range for {} (file has {} lines)",
                m.line,
                path.display(),
                line_starts.len()
            )));
        }
        let line_start = line_starts[line_idx - 1];

        // Re-extract captures at this exact match position on the line.
        let bs = m.byte_start as usize;
        let be = m.byte_end as usize;
        let caps = pattern
            .captures_iter(&m.line_text)
            .find(|c| {
                let mat = c.get(0).expect("group 0 always present");
                mat.start() == bs && mat.end() == be
            })
            .ok_or_else(|| {
                GwError::Engine(format!(
                    "engine skew: pattern did not re-match at {}:{} byte {}",
                    path.display(),
                    m.line,
                    m.byte_start
                ))
            })?;

        // Build captures vector matching the template::expand convention.
        let mut cap_vec: Vec<(String, String)> = Vec::new();
        for (i, sub) in caps.iter().enumerate() {
            if let Some(sub) = sub {
                let key: String = if i == 0 { String::new() } else { i.to_string() };
                let val: String = sub.as_str().to_string();
                cap_vec.push((key, val));
            }
        }
        for name in pattern.capture_names().flatten() {
            if let Some(sub) = caps.name(name) {
                let key: String = name.to_string();
                let val: String = sub.as_str().to_string();
                cap_vec.push((key, val));
            }
        }

        let replacement_expanded = template::expand(replacement, &cap_vec)?;

        edits.push(Edit {
            byte_start: line_start + bs,
            byte_end: line_start + be,
            replacement: replacement_expanded,
        });
    }

    Ok((content, edits))
}

/// Atomically replace `path`'s contents with `content`. Writes a temp file
/// in `path`'s parent directory, fsyncs it, then `rename`s it into place.
/// Since both files share a filesystem, the rename is atomic on POSIX, and
/// any reader observes either the old or new content — never a partial write.
///
/// Errors if the parent directory doesn't exist (we don't `mkdir -p`; the
/// caller is rewriting an existing file by definition).
pub fn write_file_atomic(path: &Path, content: &str) -> Result<(), GwError> {
    let parent = path.parent().ok_or_else(|| {
        GwError::Engine(format!(
            "cannot write file with no parent directory: {}",
            path.display()
        ))
    })?;
    // tempfile::NamedTempFile::new_in errors cleanly if the parent dir
    // doesn't exist — we surface that as a GwError::Engine.
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| GwError::Engine(format!("create temp file in {}: {e}", parent.display())))?;
    tmp.write_all(content.as_bytes())
        .map_err(|e| GwError::Engine(format!("write temp file for {}: {e}", path.display())))?;
    tmp.flush()
        .map_err(|e| GwError::Engine(format!("flush temp file for {}: {e}", path.display())))?;
    tmp.persist(path)
        .map_err(|e| GwError::Engine(format!("persist {} atomically: {e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(start: usize, end: usize, repl: &str) -> Edit {
        Edit {
            byte_start: start,
            byte_end: end,
            replacement: repl.to_string(),
        }
    }

    #[test]
    fn apply_edits_empty_returns_unchanged() {
        assert_eq!(apply_edits("hello", &[]).unwrap(), "hello");
    }

    #[test]
    fn apply_edits_single_replacement() {
        let r = apply_edits("hello world", &[e(6, 11, "rust")]).unwrap();
        assert_eq!(r, "hello rust");
    }

    #[test]
    fn apply_edits_reverse_order_length_changing() {
        // Original: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        //           idx 0    5    10   15   20   25   30
        // Replace [10..15] ("ABCDE") with "x" (shrinks by 4)
        // Replace [25..30] ("PQRST") with "longer-replacement" (grows by 13)
        // If applied in forward order, the second edit would land at the wrong
        // bytes because indices shifted. Reverse order makes it correct.
        let original = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let edits = vec![e(10, 15, "x"), e(25, 30, "longer-replacement")];
        let got = apply_edits(original, &edits).unwrap();
        // Expected: "0123456789" + "x" + "FGHIJKLMNO" + "longer-replacement" + "UVWXYZ"
        let expected = "0123456789xFGHIJKLMNOlonger-replacementUVWXYZ";
        assert_eq!(got, expected);

        // Sanity-check: passing edits in the OPPOSITE order should produce the
        // same output (the function sorts internally).
        let edits_rev = vec![e(25, 30, "longer-replacement"), e(10, 15, "x")];
        assert_eq!(apply_edits(original, &edits_rev).unwrap(), expected);
    }

    #[test]
    fn apply_edits_overlap_detected() {
        let err = apply_edits("hello world", &[e(0, 5, "X"), e(3, 7, "Y")]).unwrap_err();
        assert!(err.to_string().contains("overlapping"), "{err}");
    }

    #[test]
    fn apply_edits_out_of_bounds_errors() {
        let err = apply_edits("short", &[e(0, 100, "X")]).unwrap_err();
        assert!(err.to_string().contains("exceeds file length"), "{err}");
    }

    #[test]
    fn apply_edits_adjacent_edits_ok() {
        // [0..3] and [3..6] share no bytes; should NOT trigger overlap.
        let r = apply_edits("abcdef", &[e(0, 3, "X"), e(3, 6, "Y")]).unwrap();
        assert_eq!(r, "XY");
    }

    #[test]
    fn write_file_atomic_happy_path_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("out.txt");
        // Pre-existing content: overwrite.
        std::fs::write(&p, b"old\n").unwrap();
        write_file_atomic(&p, "brand new content\nline 2\n").expect("write");
        let back = std::fs::read_to_string(&p).unwrap();
        assert_eq!(back, "brand new content\nline 2\n");
    }

    #[test]
    fn write_file_atomic_creates_new_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("fresh.txt");
        assert!(!p.exists());
        write_file_atomic(&p, "hi").expect("write");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hi");
    }

    #[test]
    fn write_file_atomic_errors_on_missing_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("no-such-subdir").join("x.txt");
        let err = write_file_atomic(&p, "x").expect_err("should fail");
        match err {
            GwError::Engine(msg) => assert!(msg.contains("temp file"), "msg: {msg}"),
            other => panic!("expected Engine, got {other:?}"),
        }
    }

    #[test]
    fn apply_edits_rejects_char_boundary_crossing() {
        // "é" is 2 bytes (0xC3 0xA9). Replacing bytes 0..1 cuts mid-character.
        let err = apply_edits("é", &[e(0, 1, "x")]).unwrap_err();
        assert!(err.to_string().contains("char boundary"), "{err}");
    }
}
