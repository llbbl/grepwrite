use clap::Parser;
use std::process::ExitCode;

mod cli;
mod snapshot;

use cli::{Cli, Command, FindArgs, FindOutputFormat, RewriteArgs, RewriteOutputFormat};
use grepwrite::errors::GwError;
use grepwrite::locate::{Locate, Query, rg::RgLocator};
use grepwrite::mutate::{apply_edits, group_matches_by_path, plan_edits_for_file};
use grepwrite::output::{caveman, compact};
use regex::Regex;

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
        Command::Undo(_) => Err(GwError::NotImplemented("gw undo")),
        Command::Snapshots => Err(GwError::NotImplemented("gw snapshots")),
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

    let matches = RgLocator.run(&query)?;

    match args.output {
        FindOutputFormat::Caveman => {
            print!("{}", caveman::render_find(&matches));
        }
        FindOutputFormat::Compact => {
            print!("{}", compact::render_find(&matches));
        }
        FindOutputFormat::Json => return Err(GwError::NotImplemented("json format")),
    }

    if matches.is_empty() { Ok(1) } else { Ok(0) }
}

fn run_rewrite(args: RewriteArgs) -> Result<i32, GwError> {
    // task #7 will implement actual writes; for now reject --apply explicitly.
    if args.apply {
        return Err(GwError::NotImplemented("--apply (task #7)"));
    }

    // Reject output formats that don't have a rewrite renderer yet. Compact
    // rewrite output is scheduled for task #7/#10 alongside the apply path.
    match args.output {
        RewriteOutputFormat::Caveman => {}
        RewriteOutputFormat::Compact => {
            return Err(GwError::NotImplemented(
                "compact rewrite output (task #7/#10)",
            ));
        }
        RewriteOutputFormat::Json => {
            return Err(GwError::NotImplemented("json rewrite output (task #10)"));
        }
        RewriteOutputFormat::Diff => {
            return Err(GwError::NotImplemented("diff rewrite output (task #10)"));
        }
    }

    // Compile the user pattern once, up front: fail fast with a clear error
    // before we spawn rg. rg will also reject the bad pattern, but our message
    // is better when the failure is purely a regex syntax issue.
    let pattern =
        Regex::new(&args.pattern).map_err(|e| GwError::Engine(format!("invalid regex: {e}")))?;

    let query = Query {
        pattern: args.pattern.clone(),
        paths: args.path.into_iter().collect(),
        type_filter: args.type_,
        globs: args.glob.into_iter().collect(),
        in_scope: args.in_scope,
        context: None,
        hidden: false,
        no_ignore: false,
    };

    let matches = RgLocator.run(&query)?;

    let grouped = group_matches_by_path(&matches);
    let mut total_edits = 0usize;
    for (path, file_matches) in &grouped {
        // Plan edits (this reads the file but never writes). We discard the
        // resulting content for the caveman renderer — it only needs counts
        // and (path, line) per match — but we still go through plan +
        // apply_edits so any error (overlap, bad capture ref, char-boundary
        // crossing) surfaces during dry-run, not just at --apply time.
        let (original, edits) =
            plan_edits_for_file(path, file_matches, &pattern, &args.replacement)?;
        let _new_content = apply_edits(&original, &edits)?;
        total_edits += edits.len();
    }

    print!("{}", caveman::render_rewrite_dry_run(&matches, total_edits));

    if total_edits == 0 { Ok(1) } else { Ok(0) }
}
