use clap::Parser;
use std::process::ExitCode;

mod cli;

use cli::{Cli, Command, FindArgs, FindOutputFormat, RewriteArgs, RewriteOutputFormat, UndoArgs};
use grepwrite::errors::GwError;
use grepwrite::locate::{Locate, Match, Query, ast_grep::AstGrepLocator, rg::RgLocator};
use grepwrite::mutate::{
    apply_edits, group_matches_by_path, plan_edits_for_file, write_file_atomic,
};
use grepwrite::output::diff::{self as diff_out, FileDiff};
use grepwrite::output::json::{self as json_out, EditPreview};
use grepwrite::output::{caveman, compact};
use grepwrite::snapshot;
use regex::Regex;
use std::path::PathBuf;
use std::process::Command as StdCommand;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match dispatch(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("gw: {e}");
            e.exit_code()
        }
    };
    ExitCode::from(code as u8)
}

fn dispatch(cli: Cli) -> Result<i32, GwError> {
    match cli.command {
        Command::Find(args) => run_find(args),
        Command::Rewrite(args) => run_rewrite(args),
        Command::Undo(args) => run_undo(args),
        Command::Snapshots => run_snapshots(),
    }
}

/// Pick the locate engine. `--in <scope>` switches to ast-grep; otherwise
/// the default rg path is unchanged.
fn locate(query: &Query) -> Result<Vec<Match>, GwError> {
    if query.in_scope.is_some() {
        AstGrepLocator.run(query)
    } else {
        RgLocator.run(query)
    }
}

fn run_find(args: FindArgs) -> Result<i32, GwError> {
    let query = Query {
        pattern: args.pattern,
        paths: args.path.into_iter().collect(),
        type_filter: args.type_,
        globs: args.glob.into_iter().collect(),
        in_scope: args.in_scope,
        context: args.context,
        hidden: args.hidden,
        no_ignore: args.no_ignore,
    };

    let engine = engine_label(&query);
    let matches = locate(&query)?;

    match args.output {
        FindOutputFormat::Caveman => {
            print!("{}", caveman::render_find(&matches));
        }
        FindOutputFormat::Compact => {
            print!("{}", compact::render_find(&matches));
        }
        FindOutputFormat::Json => {
            print!("{}", json_out::render_find(&matches, engine)?);
        }
    }

    if matches.is_empty() { Ok(1) } else { Ok(0) }
}

/// Engine string for the JSON envelope. Mirrors the dispatch in `locate()`.
fn engine_label(query: &Query) -> &'static str {
    if query.in_scope.is_some() {
        "ast-grep"
    } else {
        "rg"
    }
}

fn run_rewrite(args: RewriteArgs) -> Result<i32, GwError> {
    // Compile the user pattern once, up front: fail fast with a clear error
    // before we spawn rg. rg will also reject the bad pattern, but our message
    // is better when the failure is purely a regex syntax issue.
    let pattern =
        Regex::new(&args.pattern).map_err(|e| GwError::Engine(format!("invalid regex: {e}")))?;

    let query = Query {
        pattern: args.pattern.clone(),
        paths: args.path.clone().into_iter().collect(),
        type_filter: args.type_.clone(),
        globs: args.glob.clone().into_iter().collect(),
        in_scope: args.in_scope.clone(),
        context: None,
        hidden: false,
        no_ignore: false,
    };

    let matches = locate(&query)?;

    if args.apply {
        run_rewrite_apply(args, &pattern, &matches)
    } else {
        run_rewrite_dry_run(args, &pattern, &matches)
    }
}

fn run_rewrite_dry_run(
    args: RewriteArgs,
    pattern: &Regex,
    matches: &[grepwrite::locate::Match],
) -> Result<i32, GwError> {
    let plan = plan_all(matches, pattern, &args.replacement)?;
    let total_edits = plan.total_edits;

    match args.output {
        RewriteOutputFormat::Caveman => {
            print!("{}", caveman::render_rewrite_dry_run(matches, total_edits));
        }
        // Compact aliases to Diff for rewrite — see RewriteOutputFormat docs.
        RewriteOutputFormat::Compact | RewriteOutputFormat::Diff => {
            print!("{}", diff_out::render_rewrite(&plan.file_diffs));
        }
        RewriteOutputFormat::Json => {
            let engine = engine_label_for_rewrite(&args);
            print!(
                "{}",
                json_out::render_rewrite(matches, &plan.edit_previews, false, None, engine)?
            );
        }
    }

    if total_edits == 0 { Ok(1) } else { Ok(0) }
}

fn engine_label_for_rewrite(args: &RewriteArgs) -> &'static str {
    if args.in_scope.is_some() {
        "ast-grep"
    } else {
        "rg"
    }
}

/// Outcome of planning every file's edits — both the byte-edits used to
/// produce post-apply content and the materialized before/after strings used
/// by the JSON and diff renderers.
struct PlanResult {
    /// (path, new_content, edit_count) — fed to apply.
    planned: Vec<(PathBuf, String, usize)>,
    /// Per-file before/after pairs for diff rendering.
    file_diffs: Vec<FileDiff>,
    /// Per-match before/after pairs for JSON rendering.
    edit_previews: Vec<EditPreview>,
    total_edits: usize,
}

/// Plan edits across every file once. Centralizes the read/plan/apply
/// pipeline so dry-run and apply both produce identical JSON/diff payloads.
fn plan_all(
    matches: &[grepwrite::locate::Match],
    pattern: &Regex,
    replacement: &str,
) -> Result<PlanResult, GwError> {
    let grouped = group_matches_by_path(matches);
    let mut planned: Vec<(PathBuf, String, usize)> = Vec::with_capacity(grouped.len());
    let mut file_diffs: Vec<FileDiff> = Vec::with_capacity(grouped.len());
    let mut edit_previews: Vec<EditPreview> = Vec::with_capacity(matches.len());
    let mut total_edits = 0usize;

    for (path, file_matches) in &grouped {
        let (original, edits) = plan_edits_for_file(path, file_matches, pattern, replacement)?;
        let new_content = apply_edits(&original, &edits)?;
        total_edits += edits.len();

        // Per-match preview: pair each Match with its planned Edit in input
        // order. plan_edits_for_file preserves order, so this is a simple zip.
        for (m, e) in file_matches.iter().zip(edits.iter()) {
            let before = original
                .get(e.byte_start..e.byte_end)
                .unwrap_or("")
                .to_string();
            edit_previews.push(EditPreview {
                path: path.clone(),
                line: m.line,
                before,
                after: e.replacement.clone(),
            });
        }

        file_diffs.push(FileDiff {
            path: path.clone(),
            before: original,
            after: new_content.clone(),
        });
        planned.push((path.clone(), new_content, edits.len()));
    }

    Ok(PlanResult {
        planned,
        file_diffs,
        edit_previews,
        total_edits,
    })
}

fn run_rewrite_apply(
    args: RewriteArgs,
    pattern: &Regex,
    matches: &[grepwrite::locate::Match],
) -> Result<i32, GwError> {
    // Anchor every relative path against the user's search root so that the
    // snapshot layer can normalize them under the repo root.
    let search_root = args.path.clone().unwrap_or_else(|| PathBuf::from("."));

    // Repo detection. With --no-snapshot, missing repo is allowed (writes
    // proceed, no undo possible). Otherwise refusal short-circuits everything.
    let repo_root: Option<PathBuf> = if args.no_snapshot {
        snapshot::try_detect_repo_root(&search_root)?
    } else {
        Some(snapshot::detect_repo_root(&search_root)?)
    };

    // Clean-tree precheck. Applies whole-repo, per spec: a dirty tree
    // anywhere is dirty for our purposes, since `git checkout <sha> -- paths`
    // could surprise the user later. Bypass with --force.
    if let Some(repo) = repo_root.as_deref()
        && !args.force
    {
        require_clean_tree(repo)?;
    }

    // Plan every edit before touching disk. If any planning step fails, no
    // files are written.
    let plan = plan_all(matches, pattern, &args.replacement)?;
    let total_edits = plan.total_edits;
    let planned = plan.planned;
    let file_diffs = plan.file_diffs;
    let edit_previews = plan.edit_previews;

    if planned.is_empty() {
        // Nothing to apply; mirror dry-run no-match exit code. Honor the
        // user's selected output format for the empty-result render.
        emit_rewrite_apply_output(&args, matches, &file_diffs, &edit_previews, None)?;
        return Ok(1);
    }

    // Snapshot CREATE (pre-write). After this point, even if writes fail
    // partway, the manifest covers all targeted paths so `gw undo` can
    // restore the originals from HEAD.
    let mut manifest = match repo_root.as_deref() {
        Some(repo) if !args.no_snapshot => {
            let paths: Vec<PathBuf> = planned.iter().map(|(p, _, _)| p.clone()).collect();
            Some(snapshot::create(
                repo,
                &paths,
                args.snapshot.clone(),
                total_edits,
            )?)
        }
        _ => {
            // Either --no-snapshot or no repo (only reachable with --no-snapshot).
            eprintln!(
                "gw: warning: no snapshot created (--no-snapshot or not in a git repo); undo not possible"
            );
            None
        }
    };

    // Per-file atomic writes. If one fails, abort the rest and remind the
    // user about the snapshot — partial state is on disk, but `gw undo`
    // covers every targeted path.
    for (path, new_content, _) in &planned {
        if let Err(e) = write_file_atomic(path, new_content) {
            let snapshot_hint = manifest
                .as_ref()
                .map(|m| {
                    format!(
                        " Snapshot id {} covers all targeted files; run `gw undo {}` to roll back.",
                        m.id, m.id
                    )
                })
                .unwrap_or_default();
            return Err(GwError::Engine(format!(
                "file write failed for '{}': {e}.{snapshot_hint}",
                path.display()
            )));
        }
    }

    // Snapshot RECORD (post-write). If this fails, writes already landed —
    // we proceed with a warning rather than abort. The manifest still exists
    // (without blob hashes), and `undo` will fall back to the HEAD-only check
    // for paths the user hasn't touched (which, immediately after apply,
    // means content == what we just wrote, NOT what HEAD has — so this
    // degraded path effectively blocks immediate undo until the user either
    // commits or reverts). We surface the warning so the user knows.
    if let (Some(ref mut m), Some(repo)) = (manifest.as_mut(), repo_root.as_deref())
        && let Err(e) = snapshot::record_applied_blobs(m, repo)
    {
        eprintln!(
            "gw: warning: failed to record post-apply blob hashes ({e}); immediate undo may be blocked. Commit or revert your changes to recover."
        );
    }

    let snapshot_id = manifest.as_ref().map(|m| m.id.as_str());
    let _ = total_edits; // referenced via caveman path below
    emit_rewrite_apply_output(&args, matches, &file_diffs, &edit_previews, snapshot_id)?;
    Ok(0)
}

/// Format-dispatch for `gw rewrite --apply` output. For caveman, prints the
/// summary trailer to stdout (existing behavior). For compact/diff, prints
/// the unified diff to stdout and the `applied (snapshot: ...)` trailer to
/// stderr — keeping stdout pure so it can be piped into `delta` / `bat
/// --diff` / `git apply` without contamination. For json, emits the full
/// envelope (which already carries `applied`/`snapshot_id`).
fn emit_rewrite_apply_output(
    args: &RewriteArgs,
    matches: &[grepwrite::locate::Match],
    file_diffs: &[FileDiff],
    edit_previews: &[EditPreview],
    snapshot_id: Option<&str>,
) -> Result<(), GwError> {
    let total_edits = edit_previews.len();
    match args.output {
        RewriteOutputFormat::Caveman => {
            print!(
                "{}",
                caveman::render_rewrite_applied(matches, total_edits, snapshot_id)
            );
        }
        RewriteOutputFormat::Compact | RewriteOutputFormat::Diff => {
            print!("{}", diff_out::render_rewrite(file_diffs));
            match snapshot_id {
                Some(id) => eprintln!("applied (snapshot: {id})"),
                None => eprintln!("applied (no snapshot)"),
            }
        }
        RewriteOutputFormat::Json => {
            let engine = engine_label_for_rewrite(args);
            print!(
                "{}",
                json_out::render_rewrite(matches, edit_previews, true, snapshot_id, engine)?
            );
        }
    }
    Ok(())
}

/// Roll back a previously-recorded snapshot. With no `--snapshot` flag,
/// targets the most recent gw snapshot in the repo.
fn run_undo(args: UndoArgs) -> Result<i32, GwError> {
    let cwd = std::env::current_dir().map_err(|e| GwError::Engine(format!("cwd: {e}")))?;
    let repo_root = snapshot::detect_repo_root(&cwd)?;

    let identifier = match args.snapshot {
        Some(s) => s,
        None => {
            let manifests = snapshot::list(&repo_root)?;
            if manifests.is_empty() {
                return Err(GwError::Snapshot("no snapshots to undo".to_string()));
            }
            manifests[0].id.clone()
        }
    };

    let manifest = snapshot::undo(&repo_root, &identifier)?;
    let n = manifest.paths.len();
    match manifest.name {
        Some(name) => println!("undone: {} '{}' ({} files restored)", manifest.id, name, n),
        None => println!("undone: {} ({} files restored)", manifest.id, n),
    }
    Ok(0)
}

/// List all gw snapshots in the current repo, newest-first. Read-only.
fn run_snapshots() -> Result<i32, GwError> {
    let cwd = std::env::current_dir().map_err(|e| GwError::Engine(format!("cwd: {e}")))?;
    let repo_root = snapshot::detect_repo_root(&cwd)?;
    let manifests = snapshot::list(&repo_root)?;

    if manifests.is_empty() {
        println!("(no gw snapshots)");
        return Ok(0);
    }

    // Determine width for the edits column so the name column lines up.
    let max_edits_width = manifests
        .iter()
        .map(|m| m.edits_count.to_string().len())
        .max()
        .unwrap_or(1);

    for m in &manifests {
        let name = m.name.as_deref().unwrap_or("-");
        // id is a fixed 26 chars (19 timestamp stem + dash + 6 short uuid).
        println!(
            "{:<26}  {}  {:>width$} edits  {}",
            m.id,
            m.created_at,
            m.edits_count,
            name,
            width = max_edits_width
        );
    }
    Ok(0)
}

/// Whole-repo dirty check via `git status --porcelain`. Refuses with
/// `ApplyRefused` (exit 4) when anything is uncommitted; tell the user how
/// to opt out.
fn require_clean_tree(repo_root: &std::path::Path) -> Result<(), GwError> {
    let output = StdCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| GwError::Engine(format!("git status: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GwError::Engine(format!(
            "git status failed: {}",
            stderr.trim()
        )));
    }
    if !output.stdout.is_empty() {
        return Err(GwError::ApplyRefused(
            "working tree is dirty; commit or stash, or pass --force".to_string(),
        ));
    }
    Ok(())
}
