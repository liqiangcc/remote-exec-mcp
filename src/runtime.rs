use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::application::PlanningService;
use crate::audit::{AuditEvent, AuditSink, JsonlAuditSink};
use crate::catalog::{Catalog, ConfigCatalog};
use crate::config::{Config, TargetTransportConfig};
use crate::domain::{ExecutionOperation, ExecutionResult, TargetId, TaskDefinition, TaskRequest, TransferSpec};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::sftp::SftpFileTransfer;
use crate::execution::ssh::SshCommandExecutor;
use crate::execution::{CommandExecutor, ExecutionLimits, FileTransfer, TransferConstraints, TransferResult};
use crate::secret::EnvSecretProvider;
use crate::transport::ssh::{SshSession, SshTransport};
use crate::transport::{ConnectionInfo, Transport};

pub struct RemoteExecRuntime {
    config: Arc<Config>,
    transport: SshTransport<EnvSecretProvider>,
    command_executor: SshCommandExecutor,
    file_transfer: SftpFileTransfer,
    audit: JsonlAuditSink,
    concurrency: Arc<Semaphore>,
}

impl RemoteExecRuntime {
    pub fn new(config: Config) -> AppResult<Self> {
        if config.runtime.max_concurrency == 0 {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "runtime.max_concurrency must be greater than zero",
            ));
        }

        let audit = JsonlAuditSink::open(&config.runtime.audit_path)?;
        let max_concurrency = config.runtime.max_concurrency;

        Ok(Self {
            config: Arc::new(config),
            transport: SshTransport::new(EnvSecretProvider),
            command_executor: SshCommandExecutor,
            file_transfer: SftpFileTransfer,
            audit,
            concurrency: Arc::new(Semaphore::new(max_concurrency)),
        })
    }

    pub fn list_targets(&self) -> Vec<TargetId> {
        ConfigCatalog::new(&self.config).list_targets()
    }

    pub fn list_tasks(&self, target: &TargetId) -> AppResult<Vec<TaskDefinition>> {
        ConfigCatalog::new(&self.config).list_tasks(target)
    }

    pub async fn check_target(&self, target: &TargetId) -> AppResult<ConnectionInfo> {
        let _permit = self.acquire().await?;
        let request_id = Uuid::new_v4().to_string();
        self.record_started(&request_id, target, "check", None)?;

        let transport = self.target_transport(target)?;
        match self.transport.check(transport).await {
            Ok(info) => {
                self.record_finished(&request_id, target, "check", None, "success", None, None)?;
                Ok(info)
            }
            Err(error) => {
                self.record_finished(&request_id, target, "check", None, "error", None, None)?;
                Err(error)
            }
        }
    }

    pub async fn run_task(
        &self,
        target: TargetId,
        task: String,
        parameters: BTreeMap<String, Value>,
    ) -> AppResult<ExecutionResult> {
        let _permit = self.acquire().await?;
        let request_id = Uuid::new_v4().to_string();
        self.record_started(&request_id, &target, "run_task", Some(&task))?;

        let request = TaskRequest {
            target: target.clone(),
            task: task.clone(),
            parameters,
        };

        let plan = match PlanningService::new(&self.config).prepare(&request) {
            Ok(plan) => plan,
            Err(error) => {
                self.record_finished(
                    &request_id,
                    &target,
                    "run_task",
                    Some(&task),
                    "denied",
                    None,
                    None,
                )?;
                return Err(error);
            }
        };

        let transport = self.target_transport(&target)?;
        let mut session = match self.transport.connect(transport).await {
            Ok(session) => session,
            Err(error) => {
                self.record_finished(
                    &request_id,
                    &target,
                    "run_task",
                    Some(&task),
                    "error",
                    None,
                    None,
                )?;
                return Err(error);
            }
        };

        let result = match plan.operation {
            ExecutionOperation::Command { command } => {
                self.command_executor
                    .execute(&mut session, &command, ExecutionLimits::new(plan.timeout_seconds))
                    .await
            }
            _ => Err(AppError::new(
                ErrorCode::InvalidTaskDefinition,
                "task planner produced a non-command operation for run_task",
            )),
        };

        match result {
            Ok(result) => {
                let outcome = if result.success { "success" } else { "failed" };
                self.record_finished(
                    &request_id,
                    &target,
                    "run_task",
                    Some(&task),
                    outcome,
                    result.exit_code,
                    Some(result.duration_ms),
                )?;
                Ok(result)
            }
            Err(error) => {
                self.record_finished(
                    &request_id,
                    &target,
                    "run_task",
                    Some(&task),
                    "error",
                    None,
                    None,
                )?;
                Err(error)
            }
        }
    }

    pub async fn upload_file(
        &self,
        target: TargetId,
        local_path: String,
        remote_path: String,
        overwrite: bool,
    ) -> AppResult<TransferResult> {
        let _permit = self.acquire().await?;
        let request_id = Uuid::new_v4().to_string();
        self.record_started(&request_id, &target, "upload_file", None)?;

        let local_path = authorize_existing_local_path(
            &local_path,
            &self.config.runtime.allowed_local_upload_roots,
        )?;
        let policy = self.target_policy_for_transfer(&target)?;
        let max_bytes = policy.max_transfer_bytes.ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                "target policy must set max_transfer_bytes before file transfer is enabled",
            )
        })?;
        let constraints = TransferConstraints {
            timeout_seconds: self.config.runtime.transfer_timeout_seconds,
            max_bytes,
            allowed_remote_roots: policy.allowed_upload_roots,
        };
        let transfer = TransferSpec {
            source: local_path.to_string_lossy().into_owned(),
            destination: remote_path,
            overwrite,
        };

        let transport = self.target_transport(&target)?;
        let mut session = self.transport.connect(transport).await?;
        let result = self.file_transfer.upload(&mut session, &transfer, constraints).await;
        self.finish_transfer_audit(&request_id, &target, "upload_file", &result)?;
        result
    }

    pub async fn download_file(
        &self,
        target: TargetId,
        remote_path: String,
        local_path: String,
        overwrite: bool,
    ) -> AppResult<TransferResult> {
        let _permit = self.acquire().await?;
        let request_id = Uuid::new_v4().to_string();
        self.record_started(&request_id, &target, "download_file", None)?;

        let local_path = authorize_local_destination(
            &local_path,
            &self.config.runtime.allowed_local_download_roots,
        )?;
        let policy = self.target_policy_for_transfer(&target)?;
        let max_bytes = policy.max_transfer_bytes.ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                "target policy must set max_transfer_bytes before file transfer is enabled",
            )
        })?;
        let constraints = TransferConstraints {
            timeout_seconds: self.config.runtime.transfer_timeout_seconds,
            max_bytes,
            allowed_remote_roots: policy.allowed_download_roots,
        };
        let transfer = TransferSpec {
            source: remote_path,
            destination: local_path.to_string_lossy().into_owned(),
            overwrite,
        };

        let transport = self.target_transport(&target)?;
        let mut session = self.transport.connect(transport).await?;
        let result = self.file_transfer.download(&mut session, &transfer, constraints).await;
        self.finish_transfer_audit(&request_id, &target, "download_file", &result)?;
        result
    }

    async fn acquire(&self) -> AppResult<OwnedSemaphorePermit> {
        self.concurrency
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AppError::new(ErrorCode::Internal, "runtime concurrency limiter is closed"))
    }

    fn target_transport(&self, target: &TargetId) -> AppResult<&TargetTransportConfig> {
        self.config.target_transport(target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })
    }

    fn target_policy_for_transfer(&self, target: &TargetId) -> AppResult<crate::domain::TargetPolicy> {
        self.config.target_policy(target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })
    }

    fn finish_transfer_audit(
        &self,
        request_id: &str,
        target: &TargetId,
        operation: &str,
        result: &AppResult<TransferResult>,
    ) -> AppResult<()> {
        self.record_finished(
            request_id,
            target,
            operation,
            None,
            if result.is_ok() { "success" } else { "error" },
            None,
            None,
        )
    }

    fn record_started(
        &self,
        request_id: &str,
        target: &TargetId,
        operation: &str,
        task: Option<&str>,
    ) -> AppResult<()> {
        self.audit.record(&AuditEvent::new(
            request_id,
            &target.0,
            operation,
            task,
            "started",
            None,
            None,
        ))
    }

    fn record_finished(
        &self,
        request_id: &str,
        target: &TargetId,
        operation: &str,
        task: Option<&str>,
        outcome: &str,
        exit_code: Option<i32>,
        duration_ms: Option<u128>,
    ) -> AppResult<()> {
        self.audit.record(&AuditEvent::new(
            request_id,
            &target.0,
            operation,
            task,
            outcome,
            exit_code,
            duration_ms,
        ))
    }
}

fn authorize_existing_local_path(raw: &str, roots: &[String]) -> AppResult<PathBuf> {
    if roots.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidConfiguration,
            "runtime.allowed_local_upload_roots must be configured before uploads are enabled",
        ));
    }
    let candidate = std::fs::canonicalize(raw).map_err(|error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local upload path: {error}"),
        )
    })?;
    ensure_local_path_in_roots(&candidate, roots)?;
    Ok(candidate)
}

fn authorize_local_destination(raw: &str, roots: &[String]) -> AppResult<PathBuf> {
    if roots.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidConfiguration,
            "runtime.allowed_local_download_roots must be configured before downloads are enabled",
        ));
    }
    let path = Path::new(raw);
    let file_name = path.file_name().ok_or_else(|| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            "local download destination must identify a file",
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local download parent: {error}"),
        )
    })?;
    ensure_local_path_in_roots(&canonical_parent, roots)?;
    Ok(canonical_parent.join(file_name))
}

fn ensure_local_path_in_roots(candidate: &Path, roots: &[String]) -> AppResult<()> {
    for root in roots {
        let canonical_root = std::fs::canonicalize(root).map_err(|error| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("failed to canonicalize configured local root {root}: {error}"),
            )
        })?;
        if candidate.starts_with(&canonical_root) {
            return Ok(());
        }
    }
    Err(AppError::new(
        ErrorCode::TransferPathDenied,
        format!("local path is outside configured roots: {}", candidate.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_upload_path_is_bounded_by_configured_roots() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("app.jar");
        std::fs::write(&file, b"jar").unwrap();
        let allowed = vec![root.path().to_string_lossy().into_owned()];
        assert_eq!(
            authorize_existing_local_path(file.to_str().unwrap(), &allowed).unwrap(),
            std::fs::canonicalize(&file).unwrap()
        );
    }

    #[test]
    fn local_upload_path_outside_root_is_denied() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let allowed = vec![root.path().to_string_lossy().into_owned()];
        let error = authorize_existing_local_path(outside.path().to_str().unwrap(), &allowed)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransferPathDenied);
    }
}
