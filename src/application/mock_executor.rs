//! Mock execution adapter for application pipeline tests.
//!
//! This module intentionally lives outside infrastructure. It validates that
//! application orchestration depends on ExecutionPort rather than SSH details.

use crate::domain::execution::{ExecutionPlan, ExecutionResult};
use crate::application::execution_port::ExecutionPort;

#[derive(Default)]
pub struct MockExecutor;

#[async_trait::async_trait]
impl ExecutionPort for MockExecutor {
    async fn execute(
        &self,
        _plan: ExecutionPlan,
    ) -> Result<ExecutionResult, crate::application::execution_port::ExecutionError> {
        Ok(ExecutionResult::success("mock execution"))
    }
}
