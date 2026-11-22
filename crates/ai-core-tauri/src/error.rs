//! Command error payload: stable code plus a redacted message.

use ai_core::output::OutputError;
use ai_core::CoreError;
use serde::{Deserialize, Serialize};

/// Error returned by a host command.
///
/// The frontend receives `code` (stable, branchable) and a redacted `message`; it
/// never receives prompts, completions or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandError {
    /// Stable `CORE_*` code.
    pub code: String,
    /// Redacted human-readable message.
    pub message: String,
    /// Whether repeating the call could succeed.
    pub retryable: bool,
}

impl CommandError {
    /// Builds a command error from a core error.
    #[must_use]
    pub fn from_core(error: &CoreError) -> Self {
        let output = OutputError::from_error(error);
        Self::from_output(&output)
    }

    /// Builds a command error from an output error already attached to an outcome.
    #[must_use]
    pub fn from_output(error: &OutputError) -> Self {
        Self {
            code: error.code.clone(),
            message: error.message.clone(),
            retryable: error.retryable,
        }
    }
}

impl From<&CoreError> for CommandError {
    fn from(error: &CoreError) -> Self {
        Self::from_core(error)
    }
}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        Self::from_core(&error)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}
