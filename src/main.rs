use clap::Parser;
use std::process::ExitCode;

mod cli;
mod mutate;
mod snapshot;

use cli::{Cli, Command, FindArgs, FindOutputFormat};
use grepwrite::errors::GwError;
use grepwrite::locate::{Locate, Query, rg::RgLocator};
use grepwrite::output::{caveman, compact};

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
        Command::Rewrite(_args) => Err(GwError::NotImplemented("gw rewrite")),
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
