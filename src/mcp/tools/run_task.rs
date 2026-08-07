use serde::{Deserialize, Serialize};

use crate::application::task_execution_service::{TaskExecutionRequest, TaskExecutionService};

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

/// MCP adapter boundary.
///
/// The adapter translates MCP input into an application request.
pub async fn run_task(request: RunTaskRequest) -> RunTaskResponse {
    let service = TaskExecutionService::new();

    let parameters = match request.parameters {
        serde_json::Value::Object(values) => values
            .into_iter()
            .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
            .collect(),
        _ => std::collections::HashMap::new(),
    };

    let _ = service
        .execute(TaskExecutionRequest {
            target: request.target.clone(),
            task: request.task.clone(),
            parameters,
        })
        .await;

    RunTaskResponse {
        execution_id: format!("pending-{}", request.task),
        status: "delegated".to_string(),
    }
}
