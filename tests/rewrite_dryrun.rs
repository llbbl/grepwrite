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
fn dryrun_caveman_emits_path_line_and_trailer() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let f = root.join("input.txt");
    let original = "foo(a)\nfoo(b)\n";
    fs::write(&f, original).unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            r"foo\((\w+)\)",
            "bar($1)",
            root.to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .assert()
        .success();

    let out = assert.get_output();
    let stdout = std::str::from_utf8(&out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();

    // Two path:line entries, then trailer.
    assert_eq!(lines.len(), 3, "got: {stdout:?}");
    assert!(lines[0].ends_with(":1"), "got: {}", lines[0]);
    assert!(lines[1].ends_with(":2"), "got: {}", lines[1]);
    assert_eq!(lines[2], "1 files, 2 edits, dry-run");

    // Critical safety invariant: dry-run must not mutate the file.
    let after = fs::read_to_string(&f).unwrap();
    assert_eq!(after, original, "dry-run wrote to disk!");
}

#[test]
fn dryrun_no_matches_emits_trailer_and_exits_one() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "nothing here\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "zzz_absent_zzz",
            "replacement",
            dir.path().to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .assert()
        .code(1);

    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert_eq!(stdout, "0 files, 0 edits, dry-run\n");
}

#[test]
fn invalid_regex_exits_three() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "x\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "[invalid",
            "x",
            dir.path().to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .assert()
        .code(3);

    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(
        stderr.contains("invalid regex") || stderr.contains("regex"),
        "stderr: {stderr}"
    );
}

#[test]
fn unknown_capture_reference_exits_three() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "foo\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar$5",
            dir.path().to_str().unwrap(),
            "-o",
            "caveman",
        ])
        .assert()
        .code(3);

    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("$5"), "stderr: {stderr}");
}

#[test]
fn apply_flag_returns_not_implemented() {
    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args(["rewrite", "foo", "bar", "--apply"])
        .assert()
        .code(70);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("--apply"), "stderr: {stderr}");
}

#[test]
fn compact_output_returns_not_implemented() {
    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args(["rewrite", "foo", "bar"])
        .assert()
        .code(70);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("compact"), "stderr: {stderr}");
}
