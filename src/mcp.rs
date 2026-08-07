use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::TargetId;
use crate::error::AppError;
use crate::runtime::RemoteExecRuntime;

#[derive(Clone)]
pub struct RemoteExecMcp {
    runtime: Arc<RemoteExecRuntime>,
}

impl RemoteExecMcp {
    pub fn new(runtime: Arc<RemoteExecRuntime>) -> Self {
        Self { runtime }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TargetArgs {
    /// Configured target identifier returned by list_targets.
    pub target: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunTaskArgs {
    /// Configured target identifier.
    pub target: String,
    /// Task name returned by list_tasks for this target.
    pub task: String,
    /// Typed task parameters. Unknown or invalid parameters are rejected by the application validator.
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UploadFileArgs {
    pub target: String,
    /// Local path readable by the MCP server. It must be inside runtime.allowed_local_upload_roots.
    pub local_path: String,
    /// Absolute remote path inside the target's allowed_upload_roots.
    pub remote_path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DownloadFileArgs {
    pub target: String,
    /// Absolute remote path inside the target's allowed_download_roots.
    pub remote_path: String,
    /// Local destination inside runtime.allowed_local_download_roots.
    pub local_path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[tool_router(server_handler)]
impl RemoteExecMcp {
    #[tool(
        description = "List configured remote targets. This is read-only and performs no network access."
    )]
    async fn list_targets(&self) -> CallToolResult {
        CallToolResult::structured(json!({
            "targets": self.runtime.list_targets().into_iter().map(|target| target.0).collect::<Vec<_>>()
        }))
    }

    #[tool(
        description = "Check whether an allowed target can be securely connected to over SSH, including host-key and authentication verification."
    )]
    async fn check_target(&self, Parameters(args): Parameters<TargetArgs>) -> CallToolResult {
        match self.runtime.check_target(&TargetId(args.target)).await {
            Ok(info) => CallToolResult::structured(json!({
                "reachable": info.reachable,
                "remote_identity": info.remote_identity
            })),
            Err(error) => tool_error(error),
        }
    }

    #[tool(
        description = "List only the declarative tasks authorized for a target. Globally defined but disallowed tasks are not exposed."
    )]
    async fn list_tasks(&self, Parameters(args): Parameters<TargetArgs>) -> CallToolResult {
        match self.runtime.list_tasks(&TargetId(args.target)) {
            Ok(tasks) => CallToolResult::structured(json!({ "tasks": tasks })),
            Err(error) => tool_error(error),
        }
    }

    #[tool(
        description = "Execute an authorized declarative task on a target. The tool never accepts a raw shell command; policy, typed validation and planning run before SSH execution."
    )]
    async fn run_task(&self, Parameters(args): Parameters<RunTaskArgs>) -> CallToolResult {
        match self
            .runtime
            .run_task(TargetId(args.target), args.task, args.parameters)
            .await
        {
            Ok(result) => CallToolResult::structured(json!({ "result": result })),
            Err(error) => tool_error(error),
        }
    }

    #[tool(
        description = "Upload one local file to an authorized remote path using SFTP. Local and remote allowlists, size limits and overwrite policy are enforced."
    )]
    async fn upload_file(&self, Parameters(args): Parameters<UploadFileArgs>) -> CallToolResult {
        match self
            .runtime
            .upload_file(
                TargetId(args.target),
                args.local_path,
                args.remote_path,
                args.overwrite,
            )
            .await
        {
            Ok(result) => CallToolResult::structured(json!({
                "bytes_transferred": result.bytes_transferred
            })),
            Err(error) => tool_error(error),
        }
    }

    #[tool(
        description = "Download one authorized remote file using SFTP. Remote and local allowlists, size limits and overwrite policy are enforced."
    )]
    async fn download_file(
        &self,
        Parameters(args): Parameters<DownloadFileArgs>,
    ) -> CallToolResult {
        match self
            .runtime
            .download_file(
                TargetId(args.target),
                args.remote_path,
                args.local_path,
                args.overwrite,
            )
            .await
        {
            Ok(result) => CallToolResult::structured(json!({
                "bytes_transferred": result.bytes_transferred
            })),
            Err(error) => tool_error(error),
        }
    }
}

fn tool_error(error: AppError) -> CallToolResult {
    CallToolResult::structured_error(json!({
        "code": error.code.as_str(),
        "message": error.message
    }))
}
