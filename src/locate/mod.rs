#![allow(dead_code)]

use anyhow::Result;
use std::path::PathBuf;

pub mod ast_grep;
pub mod rg;

#[derive(Debug, Clone)]
pub struct Query {
    pub pattern: String,
    pub path: Option<PathBuf>,
    pub type_: Option<String>,
    pub glob: Option<String>,
    pub in_scope: Option<String>,
    pub context: Option<u32>,
    pub hidden: bool,
    pub no_ignore: bool,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_text: String,
    /// name -> value; empty string name for numbered captures
    pub captures: Vec<(String, String)>,
}

pub trait Locate {
    fn run(&self, query: &Query) -> Result<Vec<Match>>;
}
