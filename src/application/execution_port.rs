//! Execution boundary between application orchestration and infrastructure.
//!
//! Application code depends on this port, not SSH or concrete executors.

use crate::domain::execution::{ExecutionPlan, ExecutionResult};

#[async_trait::async_trait]
pub trait ExecutionPort: Send + Sync {
    async fn execute(&self, plan: ExecutionPlan) -> Result<ExecutionResult, ExecutionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("execution backend is not configured")]
    NotConfigured,
}

/// Test implementation used by application-layer tests.
pub struct NoopExecutor;

#[async_trait::async_trait]
impl ExecutionPort for NoopExecutor {
    async fn execute(&self, _plan: ExecutionPlan) -> Result<ExecutionResult, ExecutionError> {
        Err(ExecutionError::NotConfigured)
    }
}
