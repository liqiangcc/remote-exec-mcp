use anyhow::Result;

use crate::domain::{CommandSpec, ExecutionResult, TransferSpec};

#[derive(Debug, Clone)]
pub struct TransferResult {
    pub bytes_transferred: u64,
}

/// Executes a resolved command through an already-established session.
pub trait CommandExecutor<S> {
    fn execute(
        &self,
        session: &mut S,
        command: &CommandSpec,
    ) -> impl std::future::Future<Output = Result<ExecutionResult>> + Send;
}

/// File movement is a separate capability from connectivity and command execution.
pub trait FileTransfer<S> {
    fn upload(
        &self,
        session: &mut S,
        transfer: &TransferSpec,
    ) -> impl std::future::Future<Output = Result<TransferResult>> + Send;

    fn download(
        &self,
        session: &mut S,
        transfer: &TransferSpec,
    ) -> impl std::future::Future<Output = Result<TransferResult>> + Send;
}
