//! Task execution pipeline boundary.
//!
//! This module defines the orchestration contract between MCP/application
//! and the execution infrastructure. Concrete policy, planning and executor
//! implementations are intentionally injected later.

use crate::application::task_execution_service::TaskExecutionRequest;
use crate::domain::execution::ExecutionResult;

#[derive(Debug, Clone)]
pub struct TaskExecutionPipeline;

impl TaskExecutionPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Pipeline order is fixed at the application boundary:
    ///
    /// Catalog -> Policy -> Validation -> Planning -> Executor -> Audit
    ///
    /// Wiring implementations is kept outside MCP adapters.
    pub async fn execute(
        &self,
        _request: TaskExecutionRequest,
    ) -> Result<ExecutionResult, PipelineError> {
        Err(PipelineError::NotConfigured)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("execution pipeline dependencies are not configured")]
    NotConfigured,
}
