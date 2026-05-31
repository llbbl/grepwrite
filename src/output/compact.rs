use crate::locate::Match;
use std::fmt::Write as _;

/// Render matches in rg-style compact format: one header per file group,
/// then `line:col: line_text` for each hit. A blank line separates groups
/// (none before the first group or after the last).
///
/// Input order is preserved. A new header is emitted whenever the path
/// changes from the previous match, so non-contiguous repeats of the same
/// file produce a fresh header — honest reflection of input order, even
/// though `RgLocator` groups by file in practice.
pub fn render_find(matches: &[Match]) -> String {
    let mut out = String::new();
    let mut prev_path: Option<&std::path::Path> = None;

    for m in matches {
        let path = m.path.as_path();
        let new_group = prev_path.is_none_or(|p| p != path);

        if new_group {
            if prev_path.is_some() {
                out.push('\n');
            }
            let _ = writeln!(out, "{}", path.display());
            prev_path = Some(path);
        }

        let _ = writeln!(out, "{}:{}: {}", m.line, m.col, m.line_text);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn m(path: &str, line: u32, col: u32, text: &str) -> Match {
        Match {
            path: PathBuf::from(path),
            line,
            col,
            byte_start: 0,
            byte_end: 0,
            line_text: text.to_string(),
            captures: vec![],
        }
    }

    #[test]
    fn render_find_empty() {
        assert_eq!(render_find(&[]), "");
    }

    #[test]
    fn render_find_single() {
        assert_eq!(
            render_find(&[m("x.rs", 42, 5, "let foo = bar;")]),
            "x.rs\n42:5: let foo = bar;\n"
        );
    }

    #[test]
    fn render_find_multiple_in_same_file() {
        let ms = vec![
            m("a.rs", 1, 1, "first"),
            m("a.rs", 3, 7, "third"),
            m("a.rs", 10, 2, "tenth"),
        ];
        assert_eq!(
            render_find(&ms),
            "a.rs\n1:1: first\n3:7: third\n10:2: tenth\n"
        );
    }

    #[test]
    fn render_find_two_files_has_blank_between_groups() {
        let ms = vec![
            m("a.rs", 1, 1, "in a"),
            m("b.rs", 2, 4, "in b"),
            m("b.rs", 5, 1, "also b"),
        ];
        assert_eq!(
            render_find(&ms),
            "a.rs\n1:1: in a\n\nb.rs\n2:4: in b\n5:1: also b\n"
        );
    }

    #[test]
    fn render_find_three_files_spacing() {
        let ms = vec![
            m("a.rs", 1, 1, "a1"),
            m("b.rs", 2, 2, "b1"),
            m("b.rs", 4, 3, "b2"),
            m("c.rs", 9, 1, "c1"),
        ];
        assert_eq!(
            render_find(&ms),
            "a.rs\n1:1: a1\n\nb.rs\n2:2: b1\n4:3: b2\n\nc.rs\n9:1: c1\n"
        );
    }

    #[test]
    fn render_find_passes_through_tabs_and_whitespace() {
        let ms = vec![m("x.rs", 1, 1, "\tlet\tx  =   1;   ")];
        assert_eq!(render_find(&ms), "x.rs\n1:1: \tlet\tx  =   1;   \n");
    }

    #[test]
    fn render_find_repeated_submatches_on_same_line() {
        // Task #2 emits one Match per submatch; compact should show the
        // same line_text repeated with different cols.
        let ms = vec![
            m("x.rs", 7, 3, "foo foo foo"),
            m("x.rs", 7, 7, "foo foo foo"),
            m("x.rs", 7, 11, "foo foo foo"),
        ];
        assert_eq!(
            render_find(&ms),
            "x.rs\n7:3: foo foo foo\n7:7: foo foo foo\n7:11: foo foo foo\n"
        );
    }

    #[test]
    fn render_find_non_contiguous_repeat_emits_new_header() {
        let ms = vec![
            m("a.rs", 1, 1, "a1"),
            m("b.rs", 2, 1, "b1"),
            m("a.rs", 3, 1, "a2"),
        ];
        assert_eq!(
            render_find(&ms),
            "a.rs\n1:1: a1\n\nb.rs\n2:1: b1\n\na.rs\n3:1: a2\n"
        );
    }
}
