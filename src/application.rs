use anyhow::Result;

use crate::domain::{ExecutionPlan, TaskRequest};

/// Converts validated, authorized intent into an executable plan.
/// Concrete task implementations live outside the domain model.
pub trait TaskPlanner {
    fn plan(&self, request: &TaskRequest) -> Result<ExecutionPlan>;
}
