//! End-to-end integration tests for `gw rewrite --apply`.
//!
//! Each test builds a tempdir-backed git repo (so the working tree the binary
//! mutates is throwaway), invokes the `gw` binary via `assert_cmd`, and
//! asserts on disk state + exit code. Tests early-return with `eprintln!`
//! when `git` or `rg` is missing, matching the pattern in the rest of the
//! integration test suite.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use grepwrite::snapshot;

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

#[test]
fn apply_writes_file_and_creates_snapshot() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo here\nfoo there\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "caveman",
        ])
        .assert()
        .success();

    let out = assert.get_output();
    let stdout = std::str::from_utf8(&out.stdout).unwrap();
    let after = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after, "bar here\nbar there\n", "file not rewritten");

    // Trailer should mention the snapshot id.
    assert!(
        stdout.contains("applied (snapshot: "),
        "expected snapshot trailer; got: {stdout:?}"
    );

    // Manifest file should exist in .git/gw-snapshots/.
    let snaps_dir = repo.join(".git").join("gw-snapshots");
    let entries: Vec<_> = fs::read_dir(&snaps_dir)
        .expect("snapshots dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one snapshot manifest");
}

#[test]
fn apply_then_immediate_undo_restores_file() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    let original = "foo\nfoo\n";
    seed_committed(repo, "a.txt", original);

    Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "caveman",
        ])
        .assert()
        .success();

    // Recover the snapshot id from the manifest dir.
    let snaps_dir = repo.join(".git").join("gw-snapshots");
    let manifest_path = fs::read_dir(&snaps_dir)
        .expect("snapshots dir")
        .filter_map(Result::ok)
        .find(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .expect("manifest file")
        .path();
    let id = manifest_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Undo immediately — no intervening commit. This is the headline workflow
    // and must succeed thanks to the recorded `applied_blobs`.
    snapshot::undo(repo, &id).expect("undo immediately after apply");

    let restored = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(restored, original, "file not restored");
}

#[test]
fn apply_refuses_dirty_tree_without_force() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo\n");
    seed_committed(repo, "other.txt", "irrelevant\n");

    // Make a different file dirty.
    fs::write(repo.join("other.txt"), "dirty\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "caveman",
        ])
        .assert()
        .code(4);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("working tree is dirty"), "stderr: {stderr}");

    // File untouched.
    let after = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after, "foo\n");
}

#[test]
fn apply_force_overrides_dirty_tree() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo\n");
    seed_committed(repo, "other.txt", "x\n");
    fs::write(repo.join("other.txt"), "dirty\n").unwrap();

    Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "--force",
            "-o",
            "caveman",
        ])
        .assert()
        .success();

    let after = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after, "bar\n");
}

#[test]
fn apply_no_snapshot_in_clean_repo_writes_and_warns() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "--no-snapshot",
            "-o",
            "caveman",
        ])
        .assert()
        .success();

    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("no snapshot created"), "stderr: {stderr}");

    let after = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(after, "bar\n");

    // No manifest should have been written.
    let snaps_dir = repo.join(".git").join("gw-snapshots");
    assert!(!snaps_dir.exists() || fs::read_dir(&snaps_dir).unwrap().next().is_none());
}

#[test]
fn apply_outside_repo_with_no_snapshot_succeeds() {
    if !rg_available() {
        eprintln!("skipping: rg not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Intentionally NO `git init`.
    fs::write(root.join("a.txt"), "foo\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            root.to_str().unwrap(),
            "--apply",
            "--no-snapshot",
            "-o",
            "caveman",
        ])
        .assert()
        .success();
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("no snapshot created"), "stderr: {stderr}");

    let after = fs::read_to_string(root.join("a.txt")).unwrap();
    assert_eq!(after, "bar\n");
}

#[test]
fn apply_outside_repo_without_no_snapshot_refuses() {
    if !rg_available() {
        eprintln!("skipping: rg not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(root.join("a.txt"), "foo\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            root.to_str().unwrap(),
            "--apply",
            "-o",
            "caveman",
        ])
        .assert()
        .code(4);
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(stderr.contains("not in a git repo"), "stderr: {stderr}");

    // File untouched.
    let after = fs::read_to_string(root.join("a.txt")).unwrap();
    assert_eq!(after, "foo\n");
}

// Silence unused warning when neither rg nor git is available — PathBuf used
// only for type inference in helpers above.
#[allow(dead_code)]
fn _path_buf_used() -> PathBuf {
    PathBuf::new()
}
