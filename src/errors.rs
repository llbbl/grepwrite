use thiserror::Error;

// Most variants are constructed in later tasks (#2–#10) once the locate /
// mutate / snapshot layers exist. Only `NotImplemented` has a caller in the
// current scaffold, so silence dead-code on the enum until then.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum GwError {
    #[error("no matches")]
    NoMatches,

    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    Engine(String),

    #[error("apply refused: {0}")]
    ApplyRefused(String),

    #[error("{0}")]
    Snapshot(String),

    /// A stub verb was invoked before its implementation landed.
    /// `&'static str` is sufficient — the value is always a fixed verb label
    /// produced at the call site (e.g. `"gw rewrite"`), never a runtime string.
    #[error("{0} is not yet implemented")]
    NotImplemented(&'static str),
}

impl GwError {
    pub fn exit_code(&self) -> i32 {
        match self {
            GwError::NoMatches => 1,
            GwError::Usage(_) => 2,
            GwError::Engine(_) => 3,
            GwError::ApplyRefused(_) => 4,
            GwError::Snapshot(_) => 5,
            // EX_SOFTWARE from sysexits.h — internal software error,
            // distinct from the usage error code (2).
            GwError::NotImplemented(_) => 70,
        }
    }
}
