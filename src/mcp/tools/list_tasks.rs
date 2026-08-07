use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::domain::TargetId;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListTasksRequest {
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskSummary {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListTasksResponse {
    pub target: String,
    pub tasks: Vec<TaskSummary>,
}

/// MCP adapter for task discovery on a specific target.
///
/// The catalog already applies target policy, so globally-defined but
/// unauthorized tasks are never advertised by this adapter.
pub fn list_tasks<C>(catalog: &C, request: ListTasksRequest) -> AppResult<ListTasksResponse>
where
    C: Catalog,
{
    let target = TargetId(request.target.clone());
    let tasks = catalog
        .list_tasks(&target)?
        .into_iter()
        .map(|task| TaskSummary {
            name: task.name,
            description: task.description,
        })
        .collect();

    Ok(ListTasksResponse {
        target: request.target,
        tasks,
    })
}
