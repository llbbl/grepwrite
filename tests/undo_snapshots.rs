//! End-to-end integration tests for `gw undo` and `gw snapshots`.
//!
//! Each test builds a tempdir-backed git repo, runs the real `gw rewrite
//! --apply` binary to populate snapshot manifests, then drives `gw undo`
//! and `gw snapshots` and asserts on stdout / disk state / exit code.

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

fn rg_available() -> bool {
    StdCommand::new("rg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_available() -> bool {
    StdCommand::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(repo: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} spawn: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(repo: &Path) {
    run_git(repo, &["init", "-q", "-b", "main"]);
    run_git(repo, &["config", "user.email", "gw-test@example.com"]);
    run_git(repo, &["config", "user.name", "gw test"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
}

fn seed_committed(repo: &Path, file: &str, content: &str) {
    fs::write(repo.join(file), content).unwrap();
    run_git(repo, &["add", file]);
    run_git(repo, &["commit", "-q", "-m", "seed"]);
}

fn apply_rewrite(repo: &Path, pattern: &str, replacement: &str, snapshot_name: Option<&str>) {
    let mut cmd = Command::cargo_bin("gw").expect("gw binary");
    cmd.current_dir(repo).args([
        "rewrite",
        pattern,
        replacement,
        repo.to_str().unwrap(),
        "--apply",
        "-o",
        "caveman",
    ]);
    if let Some(name) = snapshot_name {
        cmd.args(["--snapshot", name]);
    }
    cmd.assert().success();
}

#[test]
fn snapshots_empty_prints_placeholder() {
    if !git_available() {
        eprintln!("skipping: git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "hi\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .arg("snapshots")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.contains("(no gw snapshots)"), "stdout: {stdout:?}");
}

#[test]
fn snapshots_lists_after_apply() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo\n");
    apply_rewrite(repo, "foo", "bar", None);

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .arg("snapshots")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    // One row per manifest; should mention edit count.
    assert!(stdout.contains("1 edits"), "stdout: {stdout:?}");
    // The id timestamp prefix should be present.
    assert!(stdout.contains("T"), "stdout: {stdout:?}");
    // No header row — only the one snapshot line plus the trailing newline.
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout:?}");
}

#[test]
fn undo_no_flag_rolls_back_most_recent() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    let original = "foo here\nfoo there\n";
    seed_committed(repo, "a.txt", original);
    apply_rewrite(repo, "foo", "bar", None);

    // Sanity: file was rewritten.
    let after_apply = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after_apply, "bar here\nbar there\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .arg("undo")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.contains("undone:"), "stdout: {stdout:?}");

    // File restored.
    let restored = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(restored, original, "file not restored");

    // Manifest file deleted.
    let snaps_dir = repo.join(".git").join("gw-snapshots");
    let remaining: Vec<_> = fs::read_dir(&snaps_dir)
        .expect("snapshots dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert!(remaining.is_empty(), "expected manifest to be deleted");
}

#[test]
fn undo_by_name_targets_specific_snapshot() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "alpha\n");
    seed_committed(repo, "b.txt", "beta\n");

    apply_rewrite(repo, "alpha", "ALPHA", Some("first"));
    // Commit the first apply so the second one isn't blocked by the
    // clean-tree precheck.
    run_git(repo, &["add", "-A"]);
    run_git(repo, &["commit", "-q", "-m", "first apply"]);
    apply_rewrite(repo, "beta", "BETA", Some("second"));

    // Undo only 'first' by name.
    Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .args(["undo", "--snapshot", "first"])
        .assert()
        .success();

    // a.txt restored, b.txt still BETA.
    let a_after = fs::read_to_string(repo.join("a.txt")).unwrap();
    let b_after = fs::read_to_string(repo.join("b.txt")).unwrap();
    assert_eq!(a_after, "alpha\n", "a.txt should be restored");
    assert_eq!(b_after, "BETA\n", "b.txt should still be rewritten");

    // `gw snapshots` should still show one snapshot ('second').
    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .arg("snapshots")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.contains("second"), "stdout: {stdout:?}");
    assert!(!stdout.contains("first"), "stdout: {stdout:?}");
}

#[test]
fn undo_outside_repo_exits_4() {
    if !git_available() {
        eprintln!("skipping: git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    // Intentionally NO git init.
    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(tmp.path())
        .arg("undo")
        .assert()
        .code(4);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("not in a git repo"), "stderr: {stderr:?}");
}

#[test]
fn undo_no_snapshots_exits_5() {
    if !git_available() {
        eprintln!("skipping: git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "hi\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .arg("undo")
        .assert()
        .code(5);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(
        stderr.contains("no snapshots to undo"),
        "stderr: {stderr:?}"
    );
}

#[test]
fn undo_unknown_identifier_exits_5() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo\n");
    apply_rewrite(repo, "foo", "bar", None);

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .current_dir(repo)
        .args(["undo", "--snapshot", "nonexistent_id_xyz"])
        .assert()
        .code(5);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(
        stderr.contains("no snapshot matching"),
        "stderr: {stderr:?}"
    );

    // File should still be rewritten (no rollback happened).
    let after = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after, "bar\n");
}
