use std::collections::BTreeMap;

use rmcp::{
    handler::server::wrapper::Parameters, schemars::JsonSchema, tool, tool_router,
    ErrorData as McpError,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::{TargetId, TaskRequest, TransferSpec};
use crate::error::{AppError, ErrorCode};
use crate::runtime::RemoteExecService;

#[derive(Clone)]
pub struct RemoteExecMcp {
    app: RemoteExecService,
}

impl RemoteExecMcp {
    pub fn new(app: RemoteExecService) -> Self {
        Self { app }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TargetParams {
    target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RunTaskParams {
    target: String,
    task: String,
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UploadFileParams {
    target: String,
    /// Local path on the MCP server host. It must be inside an operator-configured local upload root.
    source: String,
    /// Absolute remote destination path. It must be inside an operator-configured remote upload root.
    destination: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadFileParams {
    target: String,
    /// Absolute remote source path. It must be inside an operator-configured remote download root.
    source: String,
    /// Local path on the MCP server host. Its parent must be inside an operator-configured local download root.
    destination: String,
    #[serde(default)]
    overwrite: bool,
}

#[tool_router(server_handler)]
impl RemoteExecMcp {
    #[tool(description = "List configured remote targets. No network connection is made.")]
    async fn list_targets(&self) -> Result<String, McpError> {
        let targets: Vec<String> = self
            .app
            .list_targets()
            .into_iter()
            .map(|target| target.0)
            .collect();
        to_json(&targets)
    }

    #[tool(
        description = "Check whether a configured target can be securely reached and authenticated over SSH."
    )]
    async fn check_target(
        &self,
        Parameters(TargetParams { target }): Parameters<TargetParams>,
    ) -> Result<String, McpError> {
        let info = self
            .app
            .check_target(&TargetId(target))
            .await
            .map_err(map_app_error)?;
        to_json(&json!({
            "reachable": info.reachable,
            "remote_identity": info.remote_identity,
        }))
    }

    #[tool(description = "List only the named tasks authorized for a target by its policy.")]
    async fn list_tasks(
        &self,
        Parameters(TargetParams { target }): Parameters<TargetParams>,
    ) -> Result<String, McpError> {
        let tasks = self
            .app
            .list_tasks(&TargetId(target))
            .map_err(map_app_error)?;
        to_json(&tasks)
    }

    #[tool(
        description = "Run an operator-defined named task with typed parameters. Arbitrary shell text is not accepted."
    )]
    async fn run_task(
        &self,
        Parameters(RunTaskParams {
            target,
            task,
            parameters,
        }): Parameters<RunTaskParams>,
    ) -> Result<String, McpError> {
        let result = self
            .app
            .run_task(&TaskRequest {
                target: TargetId(target),
                task,
                parameters,
            })
            .await
            .map_err(map_app_error)?;
        to_json(&result)
    }

    #[tool(
        description = "Upload one local file over SFTP within configured local/remote roots and size limits."
    )]
    async fn upload_file(
        &self,
        Parameters(UploadFileParams {
            target,
            source,
            destination,
            overwrite,
        }): Parameters<UploadFileParams>,
    ) -> Result<String, McpError> {
        let result = self
            .app
            .upload_file(
                &TargetId(target),
                &TransferSpec {
                    source,
                    destination,
                    overwrite,
                },
            )
            .await
            .map_err(map_app_error)?;
        to_json(&json!({ "bytes_transferred": result.bytes_transferred }))
    }

    #[tool(
        description = "Download one remote file over SFTP within configured remote/local roots and size limits."
    )]
    async fn download_file(
        &self,
        Parameters(DownloadFileParams {
            target,
            source,
            destination,
            overwrite,
        }): Parameters<DownloadFileParams>,
    ) -> Result<String, McpError> {
        let result = self
            .app
            .download_file(
                &TargetId(target),
                &TransferSpec {
                    source,
                    destination,
                    overwrite,
                },
            )
            .await
            .map_err(map_app_error)?;
        to_json(&json!({ "bytes_transferred": result.bytes_transferred }))
    }
}

fn to_json(value: &impl serde::Serialize) -> Result<String, McpError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| McpError::internal_error(error.to_string(), None))
}

fn map_app_error(error: AppError) -> McpError {
    let data = Some(json!({ "code": error.code.as_str() }));
    match error.code {
        ErrorCode::InvalidRequest
        | ErrorCode::UnknownTarget
        | ErrorCode::UnknownTask
        | ErrorCode::TaskNotAllowed
        | ErrorCode::InvalidParameter
        | ErrorCode::TransferTooLarge
        | ErrorCode::TransferPathDenied
        | ErrorCode::DestinationExists => McpError::invalid_params(error.message, data),
        _ => McpError::internal_error(error.message, data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_error_code_is_preserved_in_mcp_error_data() {
        let error = map_app_error(AppError::new(ErrorCode::TaskNotAllowed, "denied"));
        assert_eq!(error.message, "denied");
        assert_eq!(error.data, Some(json!({ "code": "task_not_allowed" })));
    }
}
