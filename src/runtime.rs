use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs;

use crate::application::PlanningService;
use crate::catalog::{Catalog, ConfigCatalog};
use crate::config::{Config, TargetConfig};
use crate::domain::{ExecutionOperation, ExecutionResult, TargetId, TaskDefinition, TaskRequest, TransferSpec};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::sftp::SftpFileTransfer;
use crate::execution::ssh::SshCommandExecutor;
use crate::execution::{CommandExecutor, ExecutionLimits, FileTransfer, TransferConstraints, TransferResult};
use crate::secret::EnvSecretProvider;
use crate::transport::ssh::SshTransport;
use crate::transport::{ConnectionInfo, Transport};

/// Application facade consumed by protocol adapters.
///
/// MCP-specific types deliberately do not appear in this layer. The facade
/// composes catalog, planning, transport and execution ports while preserving
/// their existing policy boundaries.
#[derive(Clone)]
pub struct RemoteExecService {
    config: Arc<Config>,
}

impl RemoteExecService {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn list_targets(&self) -> Vec<TargetId> {
        ConfigCatalog::new(&self.config).list_targets()
    }

    pub fn list_tasks(&self, target: &TargetId) -> AppResult<Vec<TaskDefinition>> {
        ConfigCatalog::new(&self.config).list_tasks(target)
    }

    pub async fn check_target(&self, target: &TargetId) -> AppResult<ConnectionInfo> {
        let target_config = self.target(target)?;
        SshTransport::new(EnvSecretProvider)
            .check(&target_config.transport)
            .await
    }

    pub async fn run_task(&self, request: &TaskRequest) -> AppResult<ExecutionResult> {
        let plan = PlanningService::new(&self.config).prepare(request)?;
        let target_config = self.target(&plan.target)?;
        let transport = SshTransport::new(EnvSecretProvider);
        let mut session = transport.connect(&target_config.transport).await?;

        match plan.operation {
            ExecutionOperation::Command { command } => {
                SshCommandExecutor
                    .execute(
                        &mut session,
                        &command,
                        ExecutionLimits::new(plan.timeout_seconds),
                    )
                    .await
            }
            _ => Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "task planner produced a non-command operation for run_task",
            )),
        }
    }

    pub async fn upload_file(
        &self,
        target: &TargetId,
        transfer: &TransferSpec,
    ) -> AppResult<TransferResult> {
        let target_config = self.target(target)?;
        let policy = self.policy(target)?;
        ensure_existing_local_path_allowed(
            &transfer.source,
            &policy.allowed_local_upload_roots,
        )
        .await?;
        let constraints = transfer_constraints(
            policy.transfer_timeout_seconds,
            policy.max_transfer_bytes,
            policy.allowed_upload_roots,
        )?;

        let transport = SshTransport::new(EnvSecretProvider);
        let mut session = transport.connect(&target_config.transport).await?;
        SftpFileTransfer
            .upload(&mut session, transfer, constraints)
            .await
    }

    pub async fn download_file(
        &self,
        target: &TargetId,
        transfer: &TransferSpec,
    ) -> AppResult<TransferResult> {
        let target_config = self.target(target)?;
        let policy = self.policy(target)?;
        ensure_local_destination_allowed(
            &transfer.destination,
            &policy.allowed_local_download_roots,
        )
        .await?;
        let constraints = transfer_constraints(
            policy.transfer_timeout_seconds,
            policy.max_transfer_bytes,
            policy.allowed_download_roots,
        )?;

        let transport = SshTransport::new(EnvSecretProvider);
        let mut session = transport.connect(&target_config.transport).await?;
        SftpFileTransfer
            .download(&mut session, transfer, constraints)
            .await
    }

    fn target(&self, target: &TargetId) -> AppResult<&TargetConfig> {
        self.config.targets.get(&target.0).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })
    }

    fn policy(&self, target: &TargetId) -> AppResult<crate::domain::TargetPolicy> {
        self.config.target_policy(target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })
    }
}

fn transfer_constraints(
    timeout_seconds: u64,
    max_transfer_bytes: Option<u64>,
    allowed_remote_roots: Vec<String>,
) -> AppResult<TransferConstraints> {
    let max_bytes = max_transfer_bytes.ok_or_else(|| {
        AppError::new(
            ErrorCode::InvalidConfiguration,
            "file transfer is disabled until max_transfer_bytes is configured",
        )
    })?;

    TransferConstraints {
        timeout_seconds,
        max_bytes,
        allowed_remote_roots,
    }
    .validate()
}

async fn ensure_existing_local_path_allowed(path: &str, roots: &[String]) -> AppResult<()> {
    if roots.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidConfiguration,
            "upload requires at least one allowed local upload root",
        ));
    }

    let candidate = fs::canonicalize(path).await.map_err(|error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local upload source {path}: {error}"),
        )
    })?;
    ensure_canonical_local_path_allowed(&candidate, roots).await
}

async fn ensure_local_destination_allowed(path: &str, roots: &[String]) -> AppResult<()> {
    if roots.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidConfiguration,
            "download requires at least one allowed local download root",
        ));
    }

    let path = Path::new(path);
    let parent = path.parent().ok_or_else(|| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            "local download destination must have a parent directory",
        )
    })?;
    let parent = fs::canonicalize(parent).await.map_err(|error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local download parent: {error}"),
        )
    })?;
    ensure_canonical_local_path_allowed(&parent, roots).await
}

async fn ensure_canonical_local_path_allowed(candidate: &Path, roots: &[String]) -> AppResult<()> {
    for root in roots {
        let root = fs::canonicalize(root).await.map_err(|error| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("failed to canonicalize configured local root {root}: {error}"),
            )
        })?;
        reject_unrestricted_local_root(&root)?;
        if candidate.starts_with(&root) {
            return Ok(());
        }
    }

    Err(AppError::new(
        ErrorCode::TransferPathDenied,
        "local path is outside configured roots",
    ))
}

fn reject_unrestricted_local_root(root: &PathBuf) -> AppResult<()> {
    if root.parent().is_none() {
        return Err(AppError::new(
            ErrorCode::InvalidConfiguration,
            "filesystem root cannot be used as an allowed local transfer root",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_upload_path_must_stay_inside_configured_root() {
        let allowed = tempfile::tempdir().unwrap();
        let denied = tempfile::tempdir().unwrap();
        let allowed_file = allowed.path().join("app.jar");
        let denied_file = denied.path().join("secret.txt");
        fs::write(&allowed_file, b"ok").await.unwrap();
        fs::write(&denied_file, b"no").await.unwrap();
        let roots = vec![allowed.path().display().to_string()];

        assert!(ensure_existing_local_path_allowed(&allowed_file.display().to_string(), &roots)
            .await
            .is_ok());
        let error = ensure_existing_local_path_allowed(&denied_file.display().to_string(), &roots)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransferPathDenied);
    }

    #[tokio::test]
    async fn local_download_destination_is_bounded_by_parent() {
        let allowed = tempfile::tempdir().unwrap();
        let denied = tempfile::tempdir().unwrap();
        let roots = vec![allowed.path().display().to_string()];

        assert!(ensure_local_destination_allowed(
            &allowed.path().join("result.log").display().to_string(),
            &roots,
        )
        .await
        .is_ok());
        let error = ensure_local_destination_allowed(
            &denied.path().join("result.log").display().to_string(),
            &roots,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransferPathDenied);
    }
}
