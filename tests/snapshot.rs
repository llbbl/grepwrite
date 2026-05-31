//! Integration tests for `grepwrite::snapshot`.
//!
//! These tests build real git repos in tempdirs. They early-return with a
//! warning if `git` isn't on PATH, matching the rg-skip pattern used by the
//! other integration tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use grepwrite::errors::GwError;
use grepwrite::snapshot::{self, Manifest};

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
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

/// Init a repo with a known identity so commits work in CI sandboxes.
fn init_repo(repo: &Path) {
    run_git(repo, &["init", "-q", "-b", "main"]);
    run_git(repo, &["config", "user.email", "gw-test@example.com"]);
    run_git(repo, &["config", "user.name", "gw test"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
}

/// Exercises the **post-commit** undo flow: snapshot, mutate, commit, then
/// undo. This is NOT the immediate-after-apply flow — `undo` currently refuses
/// when covered paths have uncommitted changes, which is what `--apply` leaves
/// behind. See the `TODO(task #7)` comment above the dirty-check in
/// `src/snapshot/mod.rs`.
#[test]
fn undo_after_user_commits_restores_baseline() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);

    // Commit a baseline file.
    let original = b"hello\nworld\n";
    fs::write(repo.join("a.txt"), original).unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "baseline"]);

    // Snapshot at clean HEAD.
    let manifest = snapshot::create(repo, &[PathBuf::from("a.txt")], Some("test-undo".into()), 1)
        .expect("create");
    assert_eq!(manifest.name.as_deref(), Some("test-undo"));
    assert_eq!(manifest.paths, vec![PathBuf::from("a.txt")]);
    assert!(!manifest.head_sha.is_empty());

    // Manifest must live under .git/gw-snapshots/.
    let manifest_path = repo
        .join(".git")
        .join("gw-snapshots")
        .join(format!("{}.json", manifest.id));
    assert!(manifest_path.exists(), "manifest not persisted");

    // Simulate the user committing the mutated output before deciding to roll
    // back. Undo refuses to clobber an uncommitted working tree, so this commit
    // is load-bearing for the test to pass.
    fs::write(repo.join("a.txt"), b"completely\ndifferent\ncontent\n").unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "gw apply"]);

    // Undo by id.
    let restored = snapshot::undo(repo, &manifest.id).expect("undo");
    assert_eq!(restored.id, manifest.id);

    let after = fs::read(repo.join("a.txt")).unwrap();
    assert_eq!(after, original, "file not restored byte-identically");

    // Manifest is removed after successful undo.
    assert!(
        !manifest_path.exists(),
        "manifest should be deleted after undo"
    );
}

/// Pins the current refuse-on-dirty behavior. This is also the exact state
/// that `gw rewrite --apply` leaves the tree in, which is why task #7 needs
/// a design decision before the snapshot-mutate-undo loop will work
/// end-to-end. See the `TODO(task #7)` comment in `src/snapshot/mod.rs`.
#[test]
fn undo_refuses_when_paths_have_uncommitted_changes() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);

    let original = b"baseline\nfile\n";
    fs::write(repo.join("a.txt"), original).unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "baseline"]);

    let manifest = snapshot::create(repo, &[PathBuf::from("a.txt")], None, 1).expect("create");

    // Mutate on disk without committing — this is exactly the state
    // `gw rewrite --apply` leaves the tree in.
    fs::write(repo.join("a.txt"), b"mutated by gw apply\n").unwrap();

    let err = snapshot::undo(repo, &manifest.id).expect_err("should refuse dirty tree");
    match err {
        GwError::Snapshot(msg) => {
            assert!(
                msg.contains("uncommitted changes"),
                "expected 'uncommitted changes' in message, got: {msg}"
            );
            assert_eq!(GwError::Snapshot(msg).exit_code(), 5);
        }
        other => panic!("expected GwError::Snapshot, got {other:?}"),
    }
}

#[test]
fn list_returns_created_snapshot_newest_first() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    fs::write(repo.join("a.txt"), b"x\n").unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "baseline"]);

    // Empty case first: list before any snapshots.
    let empty = snapshot::list(repo).expect("list (empty)");
    assert!(empty.is_empty(), "expected no snapshots, got {empty:?}");

    let m1 = snapshot::create(repo, &[PathBuf::from("a.txt")], Some("one".into()), 1)
        .expect("create #1");

    let listed = snapshot::list(repo).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, m1.id);
    assert_eq!(listed[0].name.as_deref(), Some("one"));
}

#[test]
fn undo_by_name_works_and_is_disambiguated_by_id() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    let original = b"baseline\n";
    fs::write(repo.join("a.txt"), original).unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "baseline"]);

    let m = snapshot::create(repo, &[PathBuf::from("a.txt")], Some("my-name".into()), 1)
        .expect("create");

    fs::write(repo.join("a.txt"), b"dirty-but-uncommitted-via-test\n").unwrap();
    // Uncommitted changes to the covered path → undo must refuse.
    let err = snapshot::undo(repo, "my-name").expect_err("should refuse");
    assert!(matches!(err, GwError::Snapshot(_)), "got {err:?}");

    // Reset working tree to baseline (no uncommitted changes), then undo by name.
    fs::write(repo.join("a.txt"), original).unwrap();
    let restored = snapshot::undo(repo, "my-name").expect("undo by name");
    assert_eq!(restored.id, m.id);
}

#[test]
fn create_outside_git_repo_returns_apply_refused() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    // No `git init` — plain dir.
    let err = snapshot::create(tmp.path(), &[], None, 0).expect_err("should refuse");
    match err {
        GwError::ApplyRefused(msg) => {
            assert!(
                msg.contains("not in a git repo"),
                "unexpected message: {msg}"
            );
            assert!(
                msg.contains("--no-snapshot"),
                "message should mention --no-snapshot: {msg}"
            );
            assert_eq!(GwError::ApplyRefused(msg).exit_code(), 4);
        }
        other => panic!("expected ApplyRefused, got {other:?}"),
    }
}

#[test]
fn create_with_empty_paths_is_legal() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    fs::write(repo.join("seed.txt"), b"seed\n").unwrap();
    run_git(repo, &["add", "seed.txt"]);
    run_git(repo, &["commit", "-q", "-m", "seed"]);

    let m: Manifest = snapshot::create(repo, &[], None, 0).expect("create empty");
    assert!(m.paths.is_empty());
    assert_eq!(m.edits_count, 0);

    // Undo on an empty manifest is a no-op file-wise but must still succeed
    // and delete the manifest.
    let _ = snapshot::undo(repo, &m.id).expect("undo empty");
    let listed = snapshot::list(repo).expect("list");
    assert!(listed.iter().all(|x| x.id != m.id));
}

#[test]
fn absolute_paths_are_normalized_to_relative_in_manifest() {
    if !git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    init_repo(repo);
    fs::write(repo.join("a.txt"), b"x\n").unwrap();
    run_git(repo, &["add", "a.txt"]);
    run_git(repo, &["commit", "-q", "-m", "baseline"]);

    let abs = repo.join("a.txt");
    let m = snapshot::create(repo, &[abs], None, 1).expect("create");
    assert_eq!(m.paths, vec![PathBuf::from("a.txt")]);
}
