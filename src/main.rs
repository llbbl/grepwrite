use clap::Parser;
use std::process::ExitCode;

mod cli;
mod errors;
mod locate;
mod mutate;
mod output;
mod snapshot;

use cli::{Cli, Command};
use errors::GwError;

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
        Command::Find(_args) => Err(GwError::NotImplemented("gw find")),
        Command::Rewrite(_args) => Err(GwError::NotImplemented("gw rewrite")),
        Command::Undo(_) => Err(GwError::NotImplemented("gw undo")),
        Command::Snapshots => Err(GwError::NotImplemented("gw snapshots")),
    }
}
