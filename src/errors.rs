use thiserror::Error;

#[derive(Debug, Error)]
pub enum GwError {
    #[error("{0}")]
    Engine(String),

    #[error("apply refused: {0}")]
    ApplyRefused(String),

    #[error("{0}")]
    Snapshot(String),
}

impl GwError {
    pub fn exit_code(&self) -> i32 {
        match self {
            GwError::Engine(_) => 3,
            GwError::ApplyRefused(_) => 4,
            GwError::Snapshot(_) => 5,
        }
    }
}
