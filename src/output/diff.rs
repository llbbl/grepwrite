//! Unified diff output (`gw rewrite` only).
//!
//! Emits vanilla unified diff with `a/<path>` / `b/<path>` headers so the
//! output is consumable by `delta`, `bat --diff`, and `git apply`. No color
//! codes are emitted — terminal styling is delegated to the consumer.
//!
//! Files with `before == after` are skipped (no-op edits should not pollute
//! the diff stream).

use similar::TextDiff;
use std::path::PathBuf;

/// One file's before/after for diff rendering. Held by value because the
/// caller has already materialized both strings during planning.
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// Render unified diffs for one or more files. Per-file diffs are
/// concatenated; each `unified_diff()` output ends with its own newline, so
/// no extra separator is inserted between files. Files with no changes are
/// omitted. Empty input → empty string.
pub fn render_rewrite(file_diffs: &[FileDiff]) -> String {
    let mut out = String::new();
    for fd in file_diffs {
        if fd.before == fd.after {
            continue;
        }
        let diff = TextDiff::from_lines(&fd.before, &fd.after);
        let a_label = format!("a/{}", fd.path.display());
        let b_label = format!("b/{}", fd.path.display());
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header(&a_label, &b_label)
            .to_string();
        out.push_str(&rendered);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_empty_output() {
        assert_eq!(render_rewrite(&[]), "");
    }

    #[test]
    fn no_change_file_skipped() {
        let fds = vec![FileDiff {
            path: PathBuf::from("x.rs"),
            before: "unchanged\n".to_string(),
            after: "unchanged\n".to_string(),
        }];
        assert_eq!(render_rewrite(&fds), "");
    }

    #[test]
    fn single_file_single_line_change() {
        let fds = vec![FileDiff {
            path: PathBuf::from("src/x.ts"),
            before: "foo(user)\n".to_string(),
            after: "bar(user)\n".to_string(),
        }];
        let out = render_rewrite(&fds);
        assert!(out.contains("--- a/src/x.ts"), "missing --- header:\n{out}");
        assert!(out.contains("+++ b/src/x.ts"), "missing +++ header:\n{out}");
        assert!(out.contains("@@"), "missing hunk header:\n{out}");
        assert!(out.contains("-foo(user)"), "missing - line:\n{out}");
        assert!(out.contains("+bar(user)"), "missing + line:\n{out}");
    }

    #[test]
    fn multiple_files_each_rendered() {
        let fds = vec![
            FileDiff {
                path: PathBuf::from("a.rs"),
                before: "alpha\n".to_string(),
                after: "ALPHA\n".to_string(),
            },
            FileDiff {
                path: PathBuf::from("b.rs"),
                before: "beta\n".to_string(),
                after: "BETA\n".to_string(),
            },
        ];
        let out = render_rewrite(&fds);
        assert!(out.contains("--- a/a.rs"));
        assert!(out.contains("--- a/b.rs"));
        assert!(out.contains("-alpha"));
        assert!(out.contains("+ALPHA"));
        assert!(out.contains("-beta"));
        assert!(out.contains("+BETA"));
    }

    #[test]
    fn unchanged_file_among_changed_ones_is_skipped() {
        let fds = vec![
            FileDiff {
                path: PathBuf::from("a.rs"),
                before: "same\n".to_string(),
                after: "same\n".to_string(),
            },
            FileDiff {
                path: PathBuf::from("b.rs"),
                before: "old\n".to_string(),
                after: "new\n".to_string(),
            },
        ];
        let out = render_rewrite(&fds);
        assert!(!out.contains("a/a.rs"), "unchanged file leaked:\n{out}");
        assert!(out.contains("--- a/b.rs"));
    }

    #[test]
    fn diff_has_no_ansi_color_codes() {
        let fds = vec![FileDiff {
            path: PathBuf::from("x.rs"),
            before: "old\n".to_string(),
            after: "new\n".to_string(),
        }];
        let out = render_rewrite(&fds);
        assert!(!out.contains('\u{1b}'), "ANSI escape leaked:\n{out:?}");
    }
}
