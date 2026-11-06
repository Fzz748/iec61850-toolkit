pub mod adapters;
pub mod app;
pub mod cli;
pub mod report;
pub mod scl;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    Edit(String),
    #[error("{0}")]
    Environment(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
