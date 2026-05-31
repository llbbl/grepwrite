//! Integration tests for the `GW_LOG`-controlled tracing subscriber.
//!
//! These verify the env-control wiring, not the exact log output format
//! (which is a `tracing-subscriber` implementation detail and could shift
//! across versions). We assert presence/absence of any log activity on
//! stderr, plus presence of a known message substring when enabled.

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
fn no_log_env_keeps_stderr_quiet() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "hello foo world\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .env_remove("GW_LOG")
        .args(["find", "foo", dir.path().to_str().unwrap()])
        .assert()
        .success();

    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    assert!(
        stderr.is_empty(),
        "expected empty stderr with GW_LOG unset, got: {stderr:?}"
    );
}

#[test]
fn gw_log_debug_emits_locate_telemetry_to_stderr() {
    if !rg_available() {
        eprintln!("skipping: `rg` not on PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "hello foo world\n").unwrap();

    let assert = Command::cargo_bin("gw")
        .expect("gw binary")
        .env("GW_LOG", "debug")
        .args(["find", "foo", dir.path().to_str().unwrap()])
        .assert()
        .success();

    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap();
    // The "rg returned matches" debug line is emitted by RgLocator::run
    // when GW_LOG enables debug-level events.
    assert!(
        stderr.contains("DEBUG") && stderr.contains("rg returned matches"),
        "expected DEBUG line about rg matches in stderr, got: {stderr:?}"
    );

    // And stdout must still carry results (logging never displaces results).
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(
        stdout.contains("a.txt"),
        "expected match output on stdout, got: {stdout:?}"
    );
}
