use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable machine-readable errors for adapters such as MCP.
///
/// Messages may evolve for humans, while `ErrorCode` values are part of the
/// public contract and should remain backward compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnknownTarget,
    UnknownTask,
    TaskNotAllowed,
    InvalidParameter,
    InvalidTaskDefinition,
    InvalidConfiguration,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnknownTarget => "unknown_target",
            Self::UnknownTask => "unknown_task",
            Self::TaskNotAllowed => "task_not_allowed",
            Self::InvalidParameter => "invalid_parameter",
            Self::InvalidTaskDefinition => "invalid_task_definition",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::UnknownTarget).unwrap(),
            "\"unknown_target\""
        );
        assert_eq!(ErrorCode::TaskNotAllowed.as_str(), "task_not_allowed");
    }

    #[test]
    fn display_keeps_code_visible() {
        let error = AppError::new(ErrorCode::InvalidParameter, "service is invalid");
        assert_eq!(error.to_string(), "invalid_parameter: service is invalid");
    }
}
