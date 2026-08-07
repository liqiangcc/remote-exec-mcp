use serde::{Deserialize, Serialize};

/// MCP input for executing an approved task.
///
/// The adapter only carries intent. Policy evaluation and execution
/// remain in the application layer.
#[derive(Debug, Deserialize)]
pub struct RunTaskRequest {
    pub target: String,
    pub task: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct RunTaskResponse {
    pub execution_id: String,
    pub status: String,
}

/// Placeholder adapter boundary for the run_task MCP tool.
///
/// The implementation will delegate to PlanningService in the next step.
pub fn run_task(request: RunTaskRequest) -> RunTaskResponse {
    RunTaskResponse {
        execution_id: format!("pending-{}", request.task),
        status: "planned".to_string(),
    }
}
