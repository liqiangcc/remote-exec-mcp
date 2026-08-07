//! Application service for task execution orchestration.
//!
//! This layer coordinates domain services. It does not execute SSH commands directly.

use crate::domain::execution::ExecutionResult;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TaskExecutionRequest {
    pub target: String,
    pub task: String,
    pub parameters: HashMap<String, String>,
}

pub struct TaskExecutionService;

impl TaskExecutionService {
    pub fn new() -> Self {
        Self
    }

    /// Orchestration entry point.
    ///
    /// Flow boundary:
    /// Catalog -> Policy -> Validation -> Planning -> Executor -> Audit
    ///
    /// This service intentionally does not know SSH, SFTP, or shell details.
    pub async fn execute(
        &self,
        _request: TaskExecutionRequest,
    ) -> Result<ExecutionResult, TaskExecutionError> {
        Err(TaskExecutionError::NotWired)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskExecutionError {
    #[error("task execution pipeline is not wired yet")]
    NotWired,
}
