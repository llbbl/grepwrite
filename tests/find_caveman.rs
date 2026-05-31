use assert_cmd::Command;
use std::fs;
use std::process::Command as StdCommand;

fn rg_available() -> bool {
    StdCommand::new("rg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn find_caveman_prints_path_line_for_matches() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("a.txt"), "alpha\nbravo needle\ncharlie\n").unwrap();
    fs::write(root.join("b.txt"), "needle one\nfiller\nneedle two\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args(["find", "needle", root.to_str().unwrap(), "-o", "caveman"])
        .assert()
        .success();

    let out = assert.get_output();
    let stdout = std::str::from_utf8(&out.stdout).unwrap();

    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();

    let a = root.join("a.txt");
    let b = root.join("b.txt");
    let mut expected = [
        format!("{}:2", a.display()),
        format!("{}:1", b.display()),
        format!("{}:3", b.display()),
    ];
    expected.sort();

    assert_eq!(
        lines,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn find_caveman_no_matches_exits_one() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("only.txt"), "nothing interesting\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "find",
            "zzz_definitely_absent_zzz",
            dir.path().to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .assert()
        .code(1);

    let out = assert.get_output();
    assert!(
        out.stdout.is_empty(),
        "expected empty stdout, got {:?}",
        out.stdout
    );
}
