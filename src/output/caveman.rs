use crate::locate::Match;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

pub fn render_find(matches: &[Match]) -> String {
    let mut out = String::new();
    for m in matches {
        let _ = writeln!(out, "{}:{}", m.path.display(), m.line);
    }
    out
}

pub fn render_rewrite_dry_run(matches: &[Match], edits_count: usize) -> String {
    let mut out = render_find(matches);
    let files = matches
        .iter()
        .map(|m| &m.path)
        .collect::<HashSet<&PathBuf>>()
        .len();
    let _ = writeln!(out, "{files} files, {edits_count} edits, dry-run");
    out
}

/// Caveman renderer for the post-apply success summary. Per-match `path:line`
/// lines (so an LLM can re-read changed locations), then a trailer encoding
/// the file/edit counts and the snapshot id (or "no snapshot" when
/// `--no-snapshot` was passed). The snapshot id lives in the trailer so the
/// per-match output stays a clean grep-style stream.
pub fn render_rewrite_applied(
    matches: &[Match],
    edits_count: usize,
    snapshot_id: Option<&str>,
) -> String {
    let mut out = render_find(matches);
    let files = matches
        .iter()
        .map(|m| &m.path)
        .collect::<HashSet<&PathBuf>>()
        .len();
    match snapshot_id {
        Some(id) => {
            let _ = writeln!(
                out,
                "{files} files, {edits_count} edits, applied (snapshot: {id})"
            );
        }
        None => {
            let _ = writeln!(
                out,
                "{files} files, {edits_count} edits, applied (no snapshot)"
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn m(path: &str, line: u32) -> Match {
        Match {
            path: PathBuf::from(path),
            line,
            col: 1,
            byte_start: 0,
            byte_end: 0,
            line_text: String::new(),
            captures: vec![],
        }
    }

    #[test]
    fn render_find_empty() {
        assert_eq!(render_find(&[]), "");
    }

    #[test]
    fn render_find_single() {
        assert_eq!(render_find(&[m("x.rs", 42)]), "x.rs:42\n");
    }

    #[test]
    fn render_find_preserves_order() {
        let ms = vec![m("b.rs", 10), m("a.rs", 1), m("b.rs", 3)];
        assert_eq!(render_find(&ms), "b.rs:10\na.rs:1\nb.rs:3\n");
    }

    #[test]
    fn render_rewrite_dry_run_counts_distinct_paths() {
        let ms = vec![m("a.rs", 1), m("a.rs", 5), m("b.rs", 2)];
        let out = render_rewrite_dry_run(&ms, 3);
        assert_eq!(out, "a.rs:1\na.rs:5\nb.rs:2\n2 files, 3 edits, dry-run\n");
    }

    #[test]
    fn render_rewrite_applied_with_snapshot() {
        let ms = vec![m("a.rs", 1), m("b.rs", 2)];
        let out = render_rewrite_applied(&ms, 2, Some("2026-05-31T09-45-12-abc123"));
        assert_eq!(
            out,
            "a.rs:1\nb.rs:2\n2 files, 2 edits, applied (snapshot: 2026-05-31T09-45-12-abc123)\n"
        );
    }

    #[test]
    fn render_rewrite_applied_no_snapshot() {
        let ms = vec![m("a.rs", 1)];
        let out = render_rewrite_applied(&ms, 1, None);
        assert_eq!(out, "a.rs:1\n1 files, 1 edits, applied (no snapshot)\n");
    }

    #[test]
    fn render_rewrite_dry_run_empty() {
        assert_eq!(
            render_rewrite_dry_run(&[], 0),
            "0 files, 0 edits, dry-run\n"
        );
    }
}
