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
fn find_compact_is_default_and_prints_grouped_output() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("a.txt"), "alpha\nbravo needle\ncharlie\n").unwrap();
    fs::write(root.join("b.txt"), "needle one\nfiller\nneedle two\n").unwrap();

    // No `-o` flag — Compact must be the default.
    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args(["find", "needle", root.to_str().unwrap()])
        .assert()
        .success();

    let out = assert.get_output();
    let stdout = std::str::from_utf8(&out.stdout).unwrap();

    let a = root.join("a.txt");
    let b = root.join("b.txt");

    // Both file headers present.
    assert!(
        stdout.contains(&format!("{}\n", a.display())),
        "missing a.txt header in:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!("{}\n", b.display())),
        "missing b.txt header in:\n{stdout}"
    );

    // Expected line:col: text hits.
    assert!(
        stdout.contains("2:7: bravo needle"),
        "missing a.txt hit in:\n{stdout}"
    );
    assert!(
        stdout.contains("1:1: needle one"),
        "missing b.txt line 1 in:\n{stdout}"
    );
    assert!(
        stdout.contains("3:1: needle two"),
        "missing b.txt line 3 in:\n{stdout}"
    );

    // Blank line separates the two file groups.
    assert!(
        stdout.contains("\n\n"),
        "expected blank line between groups in:\n{stdout}"
    );
}

#[test]
fn find_compact_no_matches_exits_one_with_empty_stdout() {
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
