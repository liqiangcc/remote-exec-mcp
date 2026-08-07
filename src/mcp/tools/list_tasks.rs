//! MCP tool definition for listing available tasks.
//!
//! This layer only adapts MCP requests/responses. Task discovery remains in
//! application/domain services.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksRequest {
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksResponse {
    pub target: String,
    pub tasks: Vec<TaskSummary>,
}

/// Adapter contract for MCP list_tasks.
///
/// The implementation should delegate to application services and must not
/// directly access SSH, filesystem, or execution infrastructure.
pub trait ListTasksTool {
    fn list_tasks(&self, request: ListTasksRequest) -> Result<ListTasksResponse, String>;
}
