#![allow(dead_code)]

use crate::errors::GwError;
use std::path::PathBuf;

pub mod ast_grep;
pub mod rg;

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub pattern: String,
    pub paths: Vec<PathBuf>,
    pub type_filter: Option<String>,
    pub globs: Vec<String>,
    pub in_scope: Option<String>,
    pub context: Option<u32>,
    pub hidden: bool,
    pub no_ignore: bool,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub path: PathBuf,
    /// 1-indexed line number.
    pub line: u32,
    /// 1-indexed byte column within the line.
    pub col: u32,
    /// Start byte offset of the match. For `RgLocator` (v0.1) this is
    /// **line-relative**, not file-relative, because rg's `--json` event stream
    /// does not report file-relative offsets. The `mutate` layer reconciles
    /// this by re-reading the file. `AstGrepLocator` will use file-relative
    /// offsets; that asymmetry is intentional for v0.1.
    pub byte_start: u64,
    /// End byte offset of the match. See [`Match::byte_start`] for the
    /// line-relative caveat in `RgLocator`.
    pub byte_end: u64,
    pub line_text: String,
    /// name -> value; empty string name for numbered captures
    pub captures: Vec<(String, String)>,
}

pub trait Locate {
    fn run(&self, query: &Query) -> Result<Vec<Match>, GwError>;
}
