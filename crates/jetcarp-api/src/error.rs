//! What an operation returns when it fails: a kind a program can branch on, a message a person
//! can read, and optionally a hint saying what to change.

use schemars::JsonSchema;
use serde::Serialize;

/// Why an operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The input is wrong: a missing or malformed argument, a value out of range.
    Invalid,
    /// Something named in the input does not exist (a file, a pattern, a CA).
    NotFound,
    /// The input is fine but disagrees with what is there (a recorded hash that does not replay).
    Conflict,
    /// Reading or writing a file failed, or a path is outside what this front end may touch.
    Io,
    /// The engine refused (a rule that does not run on this tiling, a command that failed).
    Engine,
    /// The request is over a limit of this front end (world size, generations per call).
    Limit,
}

/// An operation's error.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
    /// What to do about it, when the message alone does not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> ApiError {
        ApiError {
            kind,
            message: message.into(),
            hint: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::Invalid, message)
    }
    pub fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::NotFound, message)
    }
    pub fn conflict(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::Conflict, message)
    }
    pub fn io(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::Io, message)
    }
    pub fn engine(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::Engine, message)
    }
    pub fn limit(message: impl Into<String>) -> ApiError {
        ApiError::new(ErrorKind::Limit, message)
    }
    /// The same error with a hint.
    pub fn hint(mut self, hint: impl Into<String>) -> ApiError {
        self.hint = Some(hint.into());
        self
    }
}

/// The message, then the hint on its own line.
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, "\n{h}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// Errors from the engine (`World::apply`, `Scene::from_json`, ...) are plain text.
impl From<String> for ApiError {
    fn from(message: String) -> ApiError {
        ApiError::engine(message)
    }
}

/// Mark plain-text errors with a kind: `parse(x).kind(ErrorKind::Invalid)?`.
pub trait ResultExt<T> {
    fn kind(self, kind: ErrorKind) -> Result<T, ApiError>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    fn kind(self, kind: ErrorKind) -> Result<T, ApiError> {
        self.map_err(|e| ApiError::new(kind, e.to_string()))
    }
}
