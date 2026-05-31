use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "gw",
    version,
    about = "grepwrite — search and safe, transactional rewrites",
    long_about = None,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read-only locate (ripgrep-style)
    Find(FindArgs),
    /// Mutating rewrite (dry-run by default)
    Rewrite(RewriteArgs),
    /// Roll back a prior gw snapshot
    Undo(UndoArgs),
    /// List gw snapshots
    Snapshots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FindOutputFormat {
    Compact,
    Caveman,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RewriteOutputFormat {
    Compact,
    Caveman,
    Json,
    /// Unified diff; only valid for `rewrite`.
    Diff,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    /// Regex pattern (or ast-grep pattern when --in is used)
    pub pattern: String,

    /// Search path (defaults to cwd)
    pub path: Option<PathBuf>,

    /// File type filter (e.g. ts, py, rs)
    #[arg(short = 't', long = "type")]
    pub type_: Option<String>,

    /// Include/exclude glob
    #[arg(short = 'g', long = "glob")]
    pub glob: Option<String>,

    /// AST scope: function|class|imports|comments — switches engine to ast-grep
    #[arg(long = "in", value_name = "SCOPE")]
    pub in_scope: Option<String>,

    /// Lines of context (forwarded to rg)
    #[arg(short = 'C', long = "context")]
    pub context: Option<u32>,

    /// Output format
    #[arg(short = 'o', long = "output", value_enum, default_value_t = FindOutputFormat::Compact)]
    pub output: FindOutputFormat,

    /// Include hidden files
    #[arg(long = "hidden")]
    pub hidden: bool,

    /// Do not respect .gitignore / .ignore
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,
}

#[derive(Debug, Args)]
pub struct RewriteArgs {
    /// Regex pattern
    pub pattern: String,

    /// Replacement; supports $1, ${name}, $$ for literal $
    pub replacement: String,

    /// Search path (defaults to cwd)
    pub path: Option<PathBuf>,

    /// Actually write changes. Without this, gw rewrite is a preview.
    #[arg(long = "apply")]
    pub apply: bool,

    /// Preview only (default). Present as a documented no-op since --apply is the
    /// only thing that triggers writes. Conflicts with --apply so that
    /// `--apply --dry-run` is a clap parse error rather than a silent write.
    #[arg(long = "dry-run", conflicts_with = "apply")]
    pub dry_run: bool,

    /// AST scope: function|class|imports|comments
    #[arg(long = "in", value_name = "SCOPE")]
    pub in_scope: Option<String>,

    /// File type filter
    #[arg(short = 't', long = "type")]
    pub type_: Option<String>,

    /// Include/exclude glob
    #[arg(short = 'g', long = "glob")]
    pub glob: Option<String>,

    /// Name this snapshot for later undo
    #[arg(long = "snapshot", value_name = "NAME")]
    pub snapshot: Option<String>,

    /// Skip snapshot creation (dangerous; no undo possible)
    #[arg(long = "no-snapshot", conflicts_with = "snapshot")]
    pub no_snapshot: bool,

    /// Output format (diff is rewrite-only)
    #[arg(short = 'o', long = "output", value_enum, default_value_t = RewriteOutputFormat::Compact)]
    pub output: RewriteOutputFormat,

    /// Allow apply on a dirty git tree
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct UndoArgs {
    /// Snapshot id or name; defaults to most recent
    #[arg(long = "snapshot", value_name = "NAME")]
    pub snapshot: Option<String>,
}
