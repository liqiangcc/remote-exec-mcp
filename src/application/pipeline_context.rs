//! Dependency context for task execution orchestration.
//!
//! This type intentionally only wires application dependencies. It does not
//! contain MCP, SSH, or transport-specific logic.

use crate::application::{DefaultTaskPlanner, PlanningService};
use crate::config::Config;
use crate::error::AppResult;

pub struct TaskExecutionContext<'a> {
    pub planning: PlanningService<'a, crate::validation::DefaultRequestValidator, crate::policy::DefaultPolicyEngine, DefaultTaskPlanner>,
}

impl<'a> TaskExecutionContext<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            planning: PlanningService::new(config),
        }
    }

    pub fn prepare(
        &self,
        request: &crate::domain::TaskRequest,
    ) -> AppResult<crate::domain::ExecutionPlan> {
        self.planning.prepare(request)
    }
}
