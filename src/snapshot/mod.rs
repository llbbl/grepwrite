//! Snapshot layer — git-only, manifest-based.
//!
//! Each snapshot records the HEAD SHA at creation time plus the set of paths
//! whose pre-edit content is recoverable from that commit. The manifest lives
//! at `<repo_root>/.git/gw-snapshots/<id>.json`.
//!
//! This module is read-only with respect to the working tree: `create` writes
//! only into `.git/`. Restoration (`undo`) is the only operation that touches
//! tracked files, and only via `git checkout <sha> -- <paths>`.
//!
//! No `git2` dependency — all git interaction is via `std::process::Command`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::GwError;

/// On-disk snapshot manifest. Persisted as
/// `<repo>/.git/gw-snapshots/<id>.json`.
///
/// Fields, in serialization order:
/// - `id`: sortable `<YYYY-MM-DDTHH-MM-SS>-<short-uuid>` stem.
/// - `name`: optional user-supplied label; ambiguous names refuse on undo.
/// - `head_sha`: HEAD at snapshot time; restoration source for `git checkout`.
/// - `paths`: repo-relative paths covered by the snapshot.
/// - `created_at`: RFC3339 UTC, authoritative for `list` ordering.
/// - `edits_count`: number of edits the snapshot was created for (informational).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: Option<String>,
    pub head_sha: String,
    pub paths: Vec<PathBuf>,
    pub created_at: String,
    pub edits_count: usize,
}

const SNAPSHOTS_SUBDIR: &str = "gw-snapshots";

/// Error message for the "not in a git repo" pre-check.
/// Must mention `--no-snapshot` so users know the escape hatch.
const NOT_IN_REPO_MSG: &str = "not in a git repo; gw requires git for snapshots. Re-run with --no-snapshot to write anyway (no undo possible).";

/// Run `git rev-parse --show-toplevel` starting in `start` and return the
/// canonical repo root. Returns `GwError::ApplyRefused` (exit 4) if `start`
/// is not inside a git repo.
pub fn detect_repo_root(start: &Path) -> Result<PathBuf, GwError> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .map_err(|e| GwError::Snapshot(format!("failed to invoke git: {e}")))?;

    if !output.status.success() {
        return Err(GwError::ApplyRefused(NOT_IN_REPO_MSG.to_string()));
    }

    let s = String::from_utf8(output.stdout)
        .map_err(|e| GwError::Snapshot(format!("git rev-parse: non-utf8 output: {e}")))?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(GwError::ApplyRefused(NOT_IN_REPO_MSG.to_string()));
    }
    Ok(PathBuf::from(trimmed))
}

/// Create a snapshot pinned at HEAD covering the given paths.
///
/// `paths` should be repo-relative; if absolute, they are stripped against
/// `repo_root` (out-of-tree paths are rejected as `GwError::Snapshot`).
/// An empty `paths` slice is legal — the manifest is still written.
pub fn create(
    repo_root: &Path,
    paths: &[PathBuf],
    name: Option<String>,
    edits_count: usize,
) -> Result<Manifest, GwError> {
    // First gate: confirm we're in a repo and normalize the root.
    let repo_root = detect_repo_root(repo_root)?;

    let head_sha = git_head_sha(&repo_root)?;
    let rel_paths = normalize_paths(&repo_root, paths)?;

    let created_at = now_rfc3339();
    let id = build_id(&created_at);

    let manifest = Manifest {
        id: id.clone(),
        name,
        head_sha,
        paths: rel_paths,
        created_at,
        edits_count,
    };

    let dir = snapshots_dir(&repo_root);
    fs::create_dir_all(&dir)
        .map_err(|e| GwError::Snapshot(format!("create snapshots dir {}: {e}", dir.display())))?;

    write_manifest_atomic(&dir, &manifest)?;
    Ok(manifest)
}

/// List all snapshots, newest-first. Returns `Ok(vec![])` if the snapshots
/// directory does not exist — "no snapshots" is not an error.
pub fn list(repo_root: &Path) -> Result<Vec<Manifest>, GwError> {
    let repo_root = detect_repo_root(repo_root)?;
    let dir = snapshots_dir(&repo_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let entries = fs::read_dir(&dir)
        .map_err(|e| GwError::Snapshot(format!("read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|e| GwError::Snapshot(format!("read entry in {}: {e}", dir.display())))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)
            .map_err(|e| GwError::Snapshot(format!("read {}: {e}", path.display())))?;
        let m: Manifest = serde_json::from_slice(&bytes)
            .map_err(|e| GwError::Snapshot(format!("parse manifest {}: {e}", path.display())))?;
        out.push(m);
    }

    // Newest-first. The id encodes a sortable ISO-8601 timestamp prefix, but
    // `created_at` is the authoritative field per the public API.
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// Restore paths in the snapshot identified by `identifier` (id or name).
/// Deletes the manifest on success.
pub fn undo(repo_root: &Path, identifier: &str) -> Result<Manifest, GwError> {
    let repo_root = detect_repo_root(repo_root)?;
    let dir = snapshots_dir(&repo_root);
    if !dir.exists() {
        return Err(GwError::Snapshot(format!(
            "no snapshot matching '{identifier}'"
        )));
    }

    let (manifest, manifest_path) = resolve_identifier(&dir, identifier)?;

    // Refuse if any covered path has uncommitted changes — would clobber.
    //
    // TODO(task #7): the immediate post-apply case will trip this check
    // because --apply leaves uncommitted edits in the working tree by design.
    // Need to decide between: (a) recording post-apply blob shas in the
    // manifest so we can distinguish gw-authored changes from user changes,
    // or (b) loosening this to refuse only when files differ from BOTH the
    // snapshot HEAD AND any blob we wrote. The current refuse-on-any-dirty
    // rule matches the spec but blocks the headline workflow until decided.
    if !manifest.paths.is_empty() {
        let mut status_cmd = Command::new("git");
        status_cmd
            .args(["status", "--porcelain", "--"])
            .current_dir(&repo_root);
        for p in &manifest.paths {
            status_cmd.arg(p);
        }
        let output = status_cmd
            .output()
            .map_err(|e| GwError::Snapshot(format!("git status: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GwError::Snapshot(format!(
                "git status failed: {}",
                stderr.trim()
            )));
        }
        if !output.stdout.is_empty() {
            return Err(GwError::Snapshot(format!(
                "refusing to undo snapshot '{}': uncommitted changes present in covered paths",
                manifest.id
            )));
        }

        let mut checkout_cmd = Command::new("git");
        checkout_cmd
            .args(["checkout", &manifest.head_sha, "--"])
            .current_dir(&repo_root);
        for p in &manifest.paths {
            checkout_cmd.arg(p);
        }
        let output = checkout_cmd
            .output()
            .map_err(|e| GwError::Snapshot(format!("git checkout: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GwError::Snapshot(format!(
                "git checkout failed: {}",
                stderr.trim()
            )));
        }
    }

    fs::remove_file(&manifest_path).map_err(|e| {
        GwError::Snapshot(format!("remove manifest {}: {e}", manifest_path.display()))
    })?;

    Ok(manifest)
}

// --- internals ---------------------------------------------------------------

fn snapshots_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".git").join(SNAPSHOTS_SUBDIR)
}

fn git_head_sha(repo_root: &Path) -> Result<String, GwError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| GwError::Snapshot(format!("git rev-parse HEAD: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GwError::Snapshot(format!(
            "git rev-parse HEAD failed: {}",
            stderr.trim()
        )));
    }
    let s = String::from_utf8(output.stdout)
        .map_err(|e| GwError::Snapshot(format!("git rev-parse HEAD: non-utf8: {e}")))?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(GwError::Snapshot(
            "git rev-parse HEAD: empty output (no commits yet?)".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn normalize_paths(repo_root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, GwError> {
    // Canonicalize the root once to handle platform quirks like macOS where
    // `/var/folders/...` (tempdir) and `/private/var/folders/...` (what git
    // returns from `rev-parse --show-toplevel`) differ by a symlink hop.
    let canon_root = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());

    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let rel = if p.is_absolute() {
            // Try the canonical-vs-canonical comparison first; fall back to
            // the as-given root in case canonicalize failed.
            let canon_p = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            canon_p
                .strip_prefix(&canon_root)
                .or_else(|_| p.strip_prefix(repo_root))
                .map_err(|_| {
                    GwError::Snapshot(format!(
                        "path {} is outside repo root {}",
                        p.display(),
                        repo_root.display()
                    ))
                })?
                .to_path_buf()
        } else {
            p.clone()
        };
        out.push(rel);
    }
    Ok(out)
}

fn write_manifest_atomic(dir: &Path, manifest: &Manifest) -> Result<(), GwError> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| {
        GwError::Snapshot(format!("create temp manifest in {}: {e}", dir.display()))
    })?;
    let json = serde_json::to_vec_pretty(manifest)
        .map_err(|e| GwError::Snapshot(format!("serialize manifest: {e}")))?;
    tmp.write_all(&json)
        .map_err(|e| GwError::Snapshot(format!("write temp manifest: {e}")))?;
    tmp.flush()
        .map_err(|e| GwError::Snapshot(format!("flush temp manifest: {e}")))?;
    let final_path = dir.join(format!("{}.json", manifest.id));
    tmp.persist(&final_path).map_err(|e| {
        GwError::Snapshot(format!("persist manifest to {}: {e}", final_path.display()))
    })?;
    Ok(())
}

fn resolve_identifier(dir: &Path, identifier: &str) -> Result<(Manifest, PathBuf), GwError> {
    let entries =
        fs::read_dir(dir).map_err(|e| GwError::Snapshot(format!("read {}: {e}", dir.display())))?;

    let mut by_id: Option<(Manifest, PathBuf)> = None;
    let mut by_name: Vec<(Manifest, PathBuf)> = Vec::new();

    for entry in entries {
        let entry = entry
            .map_err(|e| GwError::Snapshot(format!("read entry in {}: {e}", dir.display())))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)
            .map_err(|e| GwError::Snapshot(format!("read {}: {e}", path.display())))?;
        let m: Manifest = serde_json::from_slice(&bytes)
            .map_err(|e| GwError::Snapshot(format!("parse manifest {}: {e}", path.display())))?;
        if m.id == identifier {
            by_id = Some((m, path));
            continue;
        }
        if m.name.as_deref() == Some(identifier) {
            by_name.push((m, path));
        }
    }

    // Prefer exact id match.
    if let Some(hit) = by_id {
        return Ok(hit);
    }
    match by_name.len() {
        0 => Err(GwError::Snapshot(format!(
            "no snapshot matching '{identifier}'"
        ))),
        1 => Ok(by_name.into_iter().next().unwrap()),
        _ => Err(GwError::Snapshot(format!(
            "ambiguous snapshot name '{identifier}', use full id"
        ))),
    }
}

// --- timestamp / id helpers --------------------------------------------------

/// `<YYYY-MM-DDTHH-MM-SS>-<short-uuid>` — sortable, filesystem-safe.
fn build_id(timestamp_rfc3339: &str) -> String {
    // Convert "2026-05-31T09:45:12Z" -> "2026-05-31T09-45-12" for the id stem.
    let stem: String = timestamp_rfc3339
        .trim_end_matches('Z')
        .chars()
        .map(|c| if c == ':' { '-' } else { c })
        .collect();
    let short: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(6)
        .collect();
    format!("{stem}-{short}")
}

/// Current UTC time as RFC3339 (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Hand-rolled to keep the dependency tree small (avoiding `time` / `chrono`).
/// Uses the proleptic Gregorian calendar via Howard Hinnant's civil-from-days
/// algorithm (see <https://howardhinnant.github.io/date_algorithms.html>),
/// which is exact for all dates we care about.
fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_rfc3339(secs)
}

fn format_rfc3339(secs_since_epoch: i64) -> String {
    let days = secs_since_epoch.div_euclid(86_400);
    let time_of_day = secs_since_epoch.rem_euclid(86_400);
    let hour = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;
    let second = (time_of_day % 60) as u32;

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Days since 1970-01-01 -> (year, month, day). Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64; // 0..=146096
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // 0..=399
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0..=365
    let mp = (5 * doy + 2) / 153; // 0..=11
    let d = doy - (153 * mp + 2) / 5 + 1; // 1..=31
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // 1..=12
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

// --- unit tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_json_roundtrip_is_stable() {
        let m = Manifest {
            id: "2026-05-31T09-45-12-abc123".into(),
            name: Some("rename-foo".into()),
            head_sha: "deadbeefcafe".into(),
            paths: vec![PathBuf::from("src/x.ts"), PathBuf::from("src/y.ts")],
            created_at: "2026-05-31T09:45:12Z".into(),
            edits_count: 3,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: Manifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn manifest_handles_no_name_and_empty_paths() {
        let m = Manifest {
            id: "2026-05-31T00-00-00-zzzzzz".into(),
            name: None,
            head_sha: "0000".into(),
            paths: vec![],
            created_at: "2026-05-31T00:00:00Z".into(),
            edits_count: 0,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: Manifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
        // Sanity: the optional `name` round-trips as null.
        assert!(json.contains("\"name\":null"));
    }

    #[test]
    fn format_rfc3339_known_epoch() {
        // Epoch.
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        // One second before epoch (negative seconds path).
        assert_eq!(format_rfc3339(-1), "1969-12-31T23:59:59Z");
        // A leap-year date (2000-03-01 is doy=60 from leap rules).
        // 2000-01-01T00:00:00Z = 946_684_800.
        assert_eq!(format_rfc3339(946_684_800), "2000-01-01T00:00:00Z");
        // 2024-02-29T12:00:00Z = 1_709_208_000.
        assert_eq!(format_rfc3339(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn build_id_shape() {
        let id = build_id("2026-05-31T09:45:12Z");
        assert!(id.starts_with("2026-05-31T09-45-12-"), "id = {id}");
        // stem (19) + dash (1) + 6 hex chars
        assert_eq!(id.len(), 19 + 1 + 6);
    }
}
