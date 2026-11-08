//! Error type shared by every command. Serialises to a plain string for the UI.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unknown game: {0}")]
    UnknownGame(String),
    #[error("{0}")]
    Invalid(String),
    #[error("no active session: {0}")]
    NoSession(&'static str),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
