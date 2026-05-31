//! End-to-end tests for the `json` and `diff` output formats across both
//! `gw find` and `gw rewrite` (dry-run and `--apply`).
//!
//! These tests use a tempdir-backed git repo for the `--apply` cases since
//! the apply path requires a clean tree and creates snapshots. Each test
//! early-returns with `eprintln!` if `rg` or `git` is missing on PATH, in
//! line with the rest of the integration suite.

use assert_cmd::Command;
use serde_json::Value;
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

#[test]
fn find_json_emits_schema_v1_envelope() {
    if !rg_available() {
        eprintln!("skipping: rg not on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "foo here\nfoo there\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args(["find", "foo", dir.path().to_str().unwrap(), "-o", "json"])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    let v: Value = serde_json::from_str(stdout).expect("valid JSON");

    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["command"], "find");
    assert_eq!(v["engine"], "rg");
    assert_eq!(v["matches"].as_array().unwrap().len(), 2);
    assert_eq!(v["summary"]["matches"], 2);
    assert_eq!(v["summary"]["files"], 1);
    // edits field always present per schema; empty for find.
    assert_eq!(v["edits"].as_array().unwrap().len(), 0);
    // captures map uses "0" key.
    assert!(v["matches"][0]["captures"]["0"].is_string());
}

#[test]
fn rewrite_dryrun_json_applied_false_snapshot_null() {
    if !rg_available() {
        eprintln!("skipping: rg not on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "foo here\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            dir.path().to_str().unwrap(),
            "-o",
            "json",
        ])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    let v: Value = serde_json::from_str(stdout).expect("valid JSON");

    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["command"], "rewrite");
    assert_eq!(v["applied"], false);
    assert!(v["snapshot_id"].is_null());
    assert_eq!(v["edits"][0]["before"], "foo");
    assert_eq!(v["edits"][0]["after"], "bar");
    assert_eq!(v["summary"]["edits"], 1);

    // Dry-run must not have mutated the file.
    let content = fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(content, "foo here\n");
}

#[test]
fn rewrite_apply_json_has_applied_true_and_snapshot_id() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo here\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "json",
        ])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    let v: Value = serde_json::from_str(stdout).expect("valid JSON");

    assert_eq!(v["command"], "rewrite");
    assert_eq!(v["applied"], true);
    let snap_id = v["snapshot_id"].as_str().expect("snapshot_id string");
    assert!(!snap_id.is_empty(), "snapshot_id should be non-empty");
    assert_eq!(v["edits"][0]["before"], "foo");
    assert_eq!(v["edits"][0]["after"], "bar");

    // Apply must have written the file.
    let content = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(content, "bar here\n");
}

#[test]
fn rewrite_dryrun_diff_emits_unified_diff() {
    if !rg_available() {
        eprintln!("skipping: rg not on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "foo here\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            dir.path().to_str().unwrap(),
            "-o",
            "diff",
        ])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(
        stdout.starts_with("--- a/"),
        "expected diff header:\n{stdout}"
    );
    assert!(stdout.contains("+++ b/"), "expected +++ header:\n{stdout}");
    assert!(stdout.contains("-foo here"), "missing - line:\n{stdout}");
    assert!(stdout.contains("+bar here"), "missing + line:\n{stdout}");
}

#[test]
fn rewrite_apply_diff_writes_diff_to_stdout_trailer_to_stderr() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo here\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "diff",
        ])
        .assert()
        .success();
    let out = assert.get_output();
    let stdout = std::str::from_utf8(&out.stdout).unwrap();
    let stderr = std::str::from_utf8(&out.stderr).unwrap();

    // stdout is pure unified diff — no trailer.
    assert!(
        stdout.starts_with("--- a/"),
        "stdout should be diff:\n{stdout}"
    );
    assert!(stdout.contains("-foo here"));
    assert!(stdout.contains("+bar here"));
    assert!(
        !stdout.contains("applied"),
        "trailer should not be on stdout:\n{stdout}"
    );

    // stderr gets the applied trailer with snapshot id.
    assert!(
        stderr.contains("applied (snapshot:"),
        "missing trailer on stderr:\n{stderr}"
    );
}

#[test]
fn rewrite_compact_aliases_to_diff_for_apply() {
    if !rg_available() || !git_available() {
        eprintln!("skipping: rg or git not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    seed_committed(repo, "a.txt", "foo here\n");

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .args([
            "rewrite",
            "foo",
            "bar",
            repo.to_str().unwrap(),
            "--apply",
            "-o",
            "compact",
        ])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(
        stdout.starts_with("--- a/"),
        "compact should alias to diff for rewrite:\n{stdout}"
    );
}
