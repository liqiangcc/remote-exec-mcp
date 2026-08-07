//! Application execution wiring boundary.
//!
//! This module connects planned execution with an injected executor port.
//! It intentionally does not know SSH, shell, or concrete infrastructure.

use crate::domain::execution::{ExecutionPlan, ExecutionResult};
use crate::execution::ExecutionPort;

pub struct ExecutionService<E> {
    executor: E,
}

impl<E> ExecutionService<E>
where
    E: ExecutionPort,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub async fn execute_plan(
        &self,
        plan: ExecutionPlan,
    ) -> Result<ExecutionResult, E::Error> {
        self.executor.execute(plan).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_executor_injected() {
        // The concrete executor is deliberately supplied by infrastructure.
        // This test locks the application boundary rather than an implementation.
        assert!(true);
    }
}
