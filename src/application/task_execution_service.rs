//! Application service for task execution orchestration.
//!
//! This layer coordinates domain services. It does not execute SSH commands directly.

use crate::domain::execution::ExecutionResult;

#[derive(Debug, Clone)]
pub struct TaskExecutionRequest {
    pub target: String,
    pub task: String,
    pub parameters: std::collections::HashMap<String, String>,
}

pub struct TaskExecutionService;

impl TaskExecutionService {
    pub fn new() -> Self {
        Self
    }

    /// Orchestration entry point.
    ///
    /// Future implementation will compose:
    /// Policy -> Validation -> Planning -> Executor -> Audit
    pub async fn execute(
        &self,
        _request: TaskExecutionRequest,
    ) -> Result<ExecutionResult, String> {
        Err("task execution service is not wired yet".to_string())
    }
}
