use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::fs;
use tokio::sync::OwnedSemaphorePermit;
use tracing::error;

use crate::application::PlanningService;
use crate::audit::{AuditEvent, AuditSink, JsonlAuditSink, NoopAuditSink};
use crate::catalog::{Catalog, ConfigCatalog};
use crate::config::{AuditConfig, Config, TargetConfig};
use crate::domain::{
    ExecutionOperation, ExecutionResult, TargetId, TaskDefinition, TaskRequest, TransferSpec,
};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::sftp::SftpFileTransfer;
use crate::execution::ssh::SshCommandExecutor;
use crate::execution::{
    CommandExecutor, ExecutionLimits, FileTransfer, TransferConstraints, TransferResult,
};
use crate::guard::OperationLimiter;
use crate::secret::EnvSecretProvider;
use crate::transport::ssh::SshTransport;
use crate::transport::{ConnectionInfo, Transport};

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Application facade consumed by protocol adapters.
///
/// MCP-specific types deliberately do not appear in this layer. The facade
/// composes catalog, planning, transport and execution ports while preserving
/// their existing policy boundaries. Cross-cutting runtime guards live here so
/// every protocol adapter shares the same admission-control and audit policy.
#[derive(Clone)]
pub struct RemoteExecService {
    config: Arc<Config>,
    limiter: OperationLimiter,
    audit: Arc<dyn AuditSink>,
}

impl RemoteExecService {
    pub fn try_new(config: Config) -> AppResult<Self> {
        let limiter = OperationLimiter::new(config.runtime.max_concurrent_operations)?;
        let audit = build_audit_sink(&config.runtime.audit)?;

        Ok(Self {
            config: Arc::new(config),
            limiter,
            audit,
        })
    }

    pub fn list_targets(&self) -> Vec<TargetId> {
        ConfigCatalog::new(&self.config).list_targets()
    }

    pub fn list_tasks(&self, target: &TargetId) -> AppResult<Vec<TaskDefinition>> {
        ConfigCatalog::new(&self.config).list_tasks(target)
    }

    pub async fn check_target(&self, target: &TargetId) -> AppResult<ConnectionInfo> {
        let request_id = next_request_id();
        let target_config = match self.target(target) {
            Ok(target_config) => target_config,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "check_target",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        let operation =
            self.begin_operation(request_id, target, "check_target", None, Vec::new(), None)?;

        let result = SshTransport::new(EnvSecretProvider)
            .check(&target_config.transport)
            .await;
        self.finish_simple(&operation, &result);
        result
    }

    pub async fn run_task(&self, request: &TaskRequest) -> AppResult<ExecutionResult> {
        let request_id = next_request_id();
        let parameter_names: Vec<String> = request.parameters.keys().cloned().collect();
        let plan = match PlanningService::new(&self.config).prepare(request) {
            Ok(plan) => plan,
            Err(app_error) => {
                let policy_decision = if app_error.code == ErrorCode::TaskNotAllowed {
                    Some("denied")
                } else {
                    None
                };
                self.record_rejected(
                    &request_id,
                    &request.target,
                    "run_task",
                    AuditSubject::for_task(&request.task, &parameter_names),
                    &app_error,
                    policy_decision,
                );
                return Err(app_error);
            }
        };
        let target_config = match self.target(&plan.target) {
            Ok(target_config) => target_config,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    &request.target,
                    "run_task",
                    AuditSubject::for_task(&request.task, &parameter_names),
                    &app_error,
                    Some("allowed"),
                );
                return Err(app_error);
            }
        };
        let operation = self.begin_operation(
            request_id,
            &request.target,
            "run_task",
            Some(&request.task),
            parameter_names,
            Some("allowed"),
        )?;

        let result = async {
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
        .await;

        self.finish_execution(&operation, &result);
        result
    }

    pub async fn upload_file(
        &self,
        target: &TargetId,
        transfer: &TransferSpec,
    ) -> AppResult<TransferResult> {
        let request_id = next_request_id();
        let target_config = match self.target(target) {
            Ok(target_config) => target_config,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "upload_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        let policy = match self.policy(target) {
            Ok(policy) => policy,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "upload_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        if let Err(app_error) =
            ensure_existing_local_path_allowed(&transfer.source, &policy.allowed_local_upload_roots)
                .await
        {
            self.record_rejected(
                &request_id,
                target,
                "upload_file",
                AuditSubject::none(),
                &app_error,
                None,
            );
            return Err(app_error);
        }
        let constraints = match transfer_constraints(
            policy.transfer_timeout_seconds,
            policy.max_transfer_bytes,
            policy.allowed_upload_roots,
        ) {
            Ok(constraints) => constraints,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "upload_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        let operation =
            self.begin_operation(request_id, target, "upload_file", None, Vec::new(), None)?;

        let result = async {
            let transport = SshTransport::new(EnvSecretProvider);
            let mut session = transport.connect(&target_config.transport).await?;
            SftpFileTransfer
                .upload(&mut session, transfer, constraints)
                .await
        }
        .await;

        self.finish_transfer(&operation, &result);
        result
    }

    pub async fn download_file(
        &self,
        target: &TargetId,
        transfer: &TransferSpec,
    ) -> AppResult<TransferResult> {
        let request_id = next_request_id();
        let target_config = match self.target(target) {
            Ok(target_config) => target_config,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "download_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        let policy = match self.policy(target) {
            Ok(policy) => policy,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "download_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        if let Err(app_error) = ensure_local_destination_allowed(
            &transfer.destination,
            &policy.allowed_local_download_roots,
        )
        .await
        {
            self.record_rejected(
                &request_id,
                target,
                "download_file",
                AuditSubject::none(),
                &app_error,
                None,
            );
            return Err(app_error);
        }
        let constraints = match transfer_constraints(
            policy.transfer_timeout_seconds,
            policy.max_transfer_bytes,
            policy.allowed_download_roots,
        ) {
            Ok(constraints) => constraints,
            Err(app_error) => {
                self.record_rejected(
                    &request_id,
                    target,
                    "download_file",
                    AuditSubject::none(),
                    &app_error,
                    None,
                );
                return Err(app_error);
            }
        };
        let operation =
            self.begin_operation(request_id, target, "download_file", None, Vec::new(), None)?;

        let result = async {
            let transport = SshTransport::new(EnvSecretProvider);
            let mut session = transport.connect(&target_config.transport).await?;
            SftpFileTransfer
                .download(&mut session, transfer, constraints)
                .await
        }
        .await;

        self.finish_transfer(&operation, &result);
        result
    }

    fn begin_operation(
        &self,
        request_id: String,
        target: &TargetId,
        operation: &str,
        task: Option<&str>,
        parameter_names: Vec<String>,
        policy_decision: Option<&str>,
    ) -> AppResult<OperationContext> {
        let permit = self.limiter.try_acquire()?;
        let context = OperationContext {
            request_id,
            target: target.0.clone(),
            operation: operation.to_owned(),
            task: task.map(str::to_owned),
            parameter_names,
            policy_decision: policy_decision.map(str::to_owned),
            started_at: Instant::now(),
            _permit: permit,
        };

        self.audit
            .record(&context.event("started", None, None, None))
            .map_err(|audit_error| {
                AppError::new(
                    ErrorCode::Internal,
                    format!("failed to persist audit start event: {audit_error}"),
                )
            })?;
        Ok(context)
    }

    fn finish_simple<T>(&self, operation: &OperationContext, result: &AppResult<T>) {
        match result {
            Ok(_) => self.record_final(operation.event("succeeded", None, None, None)),
            Err(app_error) => {
                self.record_final(operation.event("failed", Some(app_error.code), None, None))
            }
        }
    }

    fn finish_execution(&self, operation: &OperationContext, result: &AppResult<ExecutionResult>) {
        match result {
            Ok(execution) => {
                let outcome = if execution.success {
                    "succeeded"
                } else {
                    "failed"
                };
                self.record_final(operation.event(outcome, None, execution.exit_code, None));
            }
            Err(app_error) => {
                self.record_final(operation.event("failed", Some(app_error.code), None, None))
            }
        }
    }

    fn finish_transfer(&self, operation: &OperationContext, result: &AppResult<TransferResult>) {
        match result {
            Ok(transfer) => self.record_final(operation.event(
                "succeeded",
                None,
                None,
                Some(transfer.bytes_transferred),
            )),
            Err(app_error) => {
                self.record_final(operation.event("failed", Some(app_error.code), None, None))
            }
        }
    }

    fn record_rejected(
        &self,
        request_id: &str,
        target: &TargetId,
        operation: &str,
        subject: AuditSubject<'_>,
        app_error: &AppError,
        policy_decision: Option<&str>,
    ) {
        let event = AuditEvent {
            request_id: request_id.to_owned(),
            timestamp: current_timestamp_millis(),
            target: target.0.clone(),
            operation: operation.to_owned(),
            task: subject.task.map(str::to_owned),
            parameter_names: subject.parameter_names.to_vec(),
            policy_decision: policy_decision.map(str::to_owned),
            outcome: "rejected".to_owned(),
            error_code: Some(app_error.code.as_str().to_owned()),
            exit_code: None,
            bytes_transferred: None,
            duration_ms: None,
        };
        self.record_final(event);
    }

    fn record_final(&self, event: AuditEvent) {
        if let Err(audit_error) = self.audit.record(&event) {
            error!(
                request_id = %event.request_id,
                operation = %event.operation,
                error = %audit_error,
                "failed to persist final audit event after operation outcome was known"
            );
        }
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

#[derive(Clone, Copy)]
struct AuditSubject<'a> {
    task: Option<&'a str>,
    parameter_names: &'a [String],
}

impl<'a> AuditSubject<'a> {
    fn none() -> Self {
        Self {
            task: None,
            parameter_names: &[],
        }
    }

    fn for_task(task: &'a str, parameter_names: &'a [String]) -> Self {
        Self {
            task: Some(task),
            parameter_names,
        }
    }
}

struct OperationContext {
    request_id: String,
    target: String,
    operation: String,
    task: Option<String>,
    parameter_names: Vec<String>,
    policy_decision: Option<String>,
    started_at: Instant,
    _permit: OwnedSemaphorePermit,
}

impl OperationContext {
    fn event(
        &self,
        outcome: &str,
        error_code: Option<ErrorCode>,
        exit_code: Option<i32>,
        bytes_transferred: Option<u64>,
    ) -> AuditEvent {
        AuditEvent {
            request_id: self.request_id.clone(),
            timestamp: current_timestamp_millis(),
            target: self.target.clone(),
            operation: self.operation.clone(),
            task: self.task.clone(),
            parameter_names: self.parameter_names.clone(),
            policy_decision: self.policy_decision.clone(),
            outcome: outcome.to_owned(),
            error_code: error_code.map(|code| code.as_str().to_owned()),
            exit_code,
            bytes_transferred,
            duration_ms: Some(self.started_at.elapsed().as_millis()),
        }
    }
}

fn build_audit_sink(config: &AuditConfig) -> AppResult<Arc<dyn AuditSink>> {
    match config {
        AuditConfig::Disabled => Ok(Arc::new(NoopAuditSink)),
        AuditConfig::Jsonl { path } => JsonlAuditSink::open(path)
            .map(|sink| Arc::new(sink) as Arc<dyn AuditSink>)
            .map_err(|audit_error| {
                AppError::new(
                    ErrorCode::InvalidConfiguration,
                    format!("failed to initialize audit sink: {audit_error}"),
                )
            }),
    }
}

fn next_request_id() -> String {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("req-{}-{sequence}", current_timestamp_millis())
}

fn current_timestamp_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned())
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

    let candidate = fs::canonicalize(path).await.map_err(|file_error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local upload source {path}: {file_error}"),
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
    let parent = fs::canonicalize(parent).await.map_err(|file_error| {
        AppError::new(
            ErrorCode::TransferPathDenied,
            format!("failed to canonicalize local download parent: {file_error}"),
        )
    })?;
    ensure_canonical_local_path_allowed(&parent, roots).await
}

async fn ensure_canonical_local_path_allowed(candidate: &Path, roots: &[String]) -> AppResult<()> {
    for root in roots {
        let root = fs::canonicalize(root).await.map_err(|file_error| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("failed to canonicalize configured local root {root}: {file_error}"),
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

fn reject_unrestricted_local_root(root: &Path) -> AppResult<()> {
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

        assert!(
            ensure_existing_local_path_allowed(&allowed_file.display().to_string(), &roots)
                .await
                .is_ok()
        );
        let app_error =
            ensure_existing_local_path_allowed(&denied_file.display().to_string(), &roots)
                .await
                .unwrap_err();
        assert_eq!(app_error.code, ErrorCode::TransferPathDenied);
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
        let app_error = ensure_local_destination_allowed(
            &denied.path().join("result.log").display().to_string(),
            &roots,
        )
        .await
        .unwrap_err();
        assert_eq!(app_error.code, ErrorCode::TransferPathDenied);
    }

    #[test]
    fn service_rejects_zero_concurrency_capacity() {
        let config: Config = serde_yaml::from_str(
            r#"
runtime:
  max_concurrent_operations: 0
"#,
        )
        .unwrap();

        let app_error = RemoteExecService::try_new(config).err().unwrap();
        assert_eq!(app_error.code, ErrorCode::InvalidConfiguration);
    }
}
