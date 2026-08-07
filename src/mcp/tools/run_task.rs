use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::task_execution_service::{TaskExecutionRequest, TaskExecutionUseCase};
use crate::error::{AppError, AppResult, ErrorCode};

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RunTaskRequest {
    pub target: String,
    pub task: String,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunTaskResponse {
    pub execution_id: String,
    pub status: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub audit_recorded: bool,
}

/// MCP adapter for executing an approved task.
///
/// This function only validates the protocol shape, translates it into an
/// application request, invokes the use case, and maps the result back to an
/// MCP-friendly response. It has no dependency on SSH or concrete executors.
pub async fn run_task<U>(service: &U, request: RunTaskRequest) -> AppResult<RunTaskResponse>
where
    U: TaskExecutionUseCase + Sync,
{
    let parameters = object_parameters(request.parameters)?;
    let outcome = service
        .execute(TaskExecutionRequest {
            target: request.target,
            task: request.task,
            parameters,
        })
        .await?;

    let result = outcome.result;
    Ok(RunTaskResponse {
        execution_id: outcome.execution_id,
        status: if result.success {
            "succeeded".to_owned()
        } else {
            "failed".to_owned()
        },
        success: result.success,
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        duration_ms: result.duration_ms,
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        audit_recorded: outcome.audit_recorded,
    })
}

fn object_parameters(value: Value) -> AppResult<BTreeMap<String, Value>> {
    match value {
        Value::Object(values) => Ok(values.into_iter().collect()),
        Value::Null => Ok(BTreeMap::new()),
        _ => Err(AppError::new(
            ErrorCode::InvalidRequest,
            "run_task parameters must be a JSON object",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use super::*;
    use crate::application::task_execution_service::{TaskExecutionOutcome, TaskExecutionRequest};
    use crate::domain::ExecutionResult;

    struct StubUseCase;

    impl TaskExecutionUseCase for StubUseCase {
        fn execute(
            &self,
            request: TaskExecutionRequest,
        ) -> impl Future<Output = AppResult<TaskExecutionOutcome>> + Send {
            async move {
                assert_eq!(request.target, "test");
                assert_eq!(request.task, "status");
                Ok(TaskExecutionOutcome {
                    execution_id: "exec-test".to_owned(),
                    result: ExecutionResult {
                        success: true,
                        exit_code: Some(0),
                        stdout: "ok".to_owned(),
                        stderr: String::new(),
                        duration_ms: 4,
                        stdout_truncated: false,
                        stderr_truncated: false,
                    },
                    audit_recorded: true,
                })
            }
        }
    }

    #[tokio::test]
    async fn delegates_to_application_use_case() {
        let response = run_task(
            &StubUseCase,
            RunTaskRequest {
                target: "test".to_owned(),
                task: "status".to_owned(),
                parameters: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        assert_eq!(response.execution_id, "exec-test");
        assert_eq!(response.status, "succeeded");
        assert_eq!(response.stdout, "ok");
    }

    #[tokio::test]
    async fn rejects_non_object_parameters_at_protocol_boundary() {
        let error = run_task(
            &StubUseCase,
            RunTaskRequest {
                target: "test".to_owned(),
                task: "status".to_owned(),
                parameters: serde_json::json!(["not", "an", "object"]),
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
}
