use grepwrite::locate::{Locate, Query, rg::RgLocator};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn rg_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn rg_locator_finds_expected_matches() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(root.join("a.txt"), "alpha\nbravo needle\ncharlie\n").unwrap();
    fs::write(root.join("b.txt"), "no match here\n").unwrap();
    fs::write(root.join("c.txt"), "needle one\nfiller\nneedle two\n").unwrap();

    let query = Query {
        pattern: "needle".into(),
        paths: vec![root.to_path_buf()],
        ..Query::default()
    };

    let matches = RgLocator.run(&query).expect("rg run");
    assert_eq!(matches.len(), 3, "expected 3 matches, got {matches:#?}");

    let mut by_path: Vec<(PathBuf, u32, String)> = matches
        .iter()
        .map(|m| (m.path.clone(), m.line, m.line_text.clone()))
        .collect();
    by_path.sort();

    let a = root.join("a.txt");
    let c = root.join("c.txt");
    assert!(by_path.contains(&(a.clone(), 2, "bravo needle".into())));
    assert!(by_path.contains(&(c.clone(), 1, "needle one".into())));
    assert!(by_path.contains(&(c.clone(), 3, "needle two".into())));

    for m in &matches {
        assert!(m.col >= 1, "col is 1-indexed");
        assert_eq!(m.captures.len(), 1);
        assert_eq!(m.captures[0].0, "");
    }
}

#[test]
fn rg_locator_returns_empty_on_no_matches() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("only.txt"), "nothing interesting\n").unwrap();

    let query = Query {
        pattern: "zzz_definitely_absent_zzz".into(),
        paths: vec![dir.path().to_path_buf()],
        ..Query::default()
    };

    let matches = RgLocator.run(&query).expect("rg run");
    assert!(matches.is_empty(), "expected empty, got {matches:#?}");
}
