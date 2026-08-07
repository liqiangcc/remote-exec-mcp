pub mod ssh;

use anyhow::Result as AnyResult;

use crate::domain::{CommandSpec, ExecutionResult, TransferSpec};
use crate::error::{AppError, AppResult, ErrorCode};

pub const DEFAULT_MAX_STDOUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub timeout_seconds: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl ExecutionLimits {
    pub const fn new(timeout_seconds: u64) -> Self {
        Self {
            timeout_seconds,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
        }
    }

    pub fn validate(self) -> AppResult<Self> {
        if self.timeout_seconds == 0 {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "command execution timeout must be greater than zero",
            ));
        }

        Ok(self)
    }
}

/// Executes a resolved command through an already-established session.
///
/// Executors receive a structured program + argv specification. They must not
/// accept model-provided raw shell text as a substitute for `CommandSpec`.
pub trait CommandExecutor<S> {
    fn execute(
        &self,
        session: &mut S,
        command: &CommandSpec,
        limits: ExecutionLimits,
    ) -> impl std::future::Future<Output = AppResult<ExecutionResult>> + Send;
}

#[derive(Debug, Clone)]
pub struct TransferResult {
    pub bytes_transferred: u64,
}

/// File movement is a separate capability from connectivity and command execution.
pub trait FileTransfer<S> {
    fn upload(
        &self,
        session: &mut S,
        transfer: &TransferSpec,
    ) -> impl std::future::Future<Output = AnyResult<TransferResult>> + Send;

    fn download(
        &self,
        session: &mut S,
        transfer: &TransferSpec,
    ) -> impl std::future::Future<Output = AnyResult<TransferResult>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_bounded() {
        let limits = ExecutionLimits::new(15);
        assert_eq!(limits.timeout_seconds, 15);
        assert_eq!(limits.max_stdout_bytes, 64 * 1024);
        assert_eq!(limits.max_stderr_bytes, 64 * 1024);
        assert_eq!(limits.validate().unwrap(), limits);
    }

    #[test]
    fn zero_timeout_is_rejected() {
        let error = ExecutionLimits::new(0).validate().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    }
}
