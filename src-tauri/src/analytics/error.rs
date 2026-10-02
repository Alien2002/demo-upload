// src-tauri/src/analytics/error.rs
//
// Every error a command can produce carries the character range it refers
// to, so the command bar can underline the exact problem.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandError {
    pub message: String,
    /// Character offsets into the command text (end exclusive).
    pub start: usize,
    pub end: usize,
}

impl CommandError {
    pub fn new(message: impl Into<String>, start: usize, end: usize) -> Self {
        Self { message: message.into(), start, end: end.max(start) }
    }

    /// Error that isn't tied to a particular part of the command.
    pub fn general(message: impl Into<String>) -> Self {
        Self { message: message.into(), start: 0, end: 0 }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {}..{})", self.message, self.start, self.end)
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::general(message)
    }
}

pub type CmdResult<T> = Result<T, CommandError>;
