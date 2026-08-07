//! Application service for task execution orchestration.
//!
//! This layer coordinates planning, execution and audit ports. It does not
//! know about MCP, SSH, SFTP, YAML, or any concrete infrastructure backend.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::application::{PlanningService, TaskPlanner};
use crate::audit::{AuditEvent, AuditSink};
use crate::domain::{ExecutionPlan, ExecutionResult, TargetId, TaskRequest};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::policy::PolicyEngine;
use crate::validation::RequestValidator;

static EXECUTION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq)]
pub struct TaskExecutionRequest {
    pub target: String,
    pub task: String,
    pub parameters: BTreeMap<String, Value>,
}

impl TaskExecutionRequest {
    fn into_domain(self) -> TaskRequest {
        TaskRequest {
            target: TargetId(self.target),
            task: self.task,
            parameters: self.parameters,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskExecutionOutcome {
    pub execution_id: String,
    pub result: ExecutionResult,
    /// Whether the terminal audit event was persisted. A pre-execution audit
    /// failure blocks execution entirely, so a returned outcome always has a
    /// successfully recorded start event.
    pub audit_recorded: bool,
}

/// Application port for preparing an authorized, validated execution plan.
pub trait PlanPreparer {
    fn prepare(&self, request: &TaskRequest) -> AppResult<ExecutionPlan>;
}

impl<'a, V, E, P> PlanPreparer for PlanningService<'a, V, E, P>
where
    V: RequestValidator,
    E: PolicyEngine,
    P: TaskPlanner,
{
    fn prepare(&self, request: &TaskRequest) -> AppResult<ExecutionPlan> {
        PlanningService::prepare(self, request)
    }
}

/// Application port for executing an already authorized `ExecutionPlan`.
/// Concrete infrastructure implementations may use SSH, local execution, or
/// another backend without changing this service.
pub trait ExecutionRunner {
    fn execute(
        &self,
        plan: &ExecutionPlan,
    ) -> impl Future<Output = AppResult<ExecutionResult>> + Send;
}

/// Protocol adapters depend on this use-case boundary rather than on the
/// concrete service type.
pub trait TaskExecutionUseCase {
    fn execute(
        &self,
        request: TaskExecutionRequest,
    ) -> impl Future<Output = AppResult<TaskExecutionOutcome>> + Send;
}

pub struct TaskExecutionService<P, R, A> {
    preparer: P,
    runner: R,
    audit: A,
}

impl<P, R, A> TaskExecutionService<P, R, A> {
    pub fn new(preparer: P, runner: R, audit: A) -> Self {
        Self {
            preparer,
            runner,
            audit,
        }
    }
}

impl<P, R, A> TaskExecutionService<P, R, A>
where
    P: PlanPreparer + Sync,
    R: ExecutionRunner + Sync,
    A: AuditSink + Sync,
{
    async fn execute_inner(&self, request: TaskExecutionRequest) -> AppResult<TaskExecutionOutcome> {
        let execution_id = next_execution_id();
        let request = request.into_domain();

        let plan = match self.preparer.prepare(&request) {
            Ok(plan) => plan,
            Err(error) => {
                let policy_decision = if error.code == ErrorCode::TaskNotAllowed {
                    Some("denied".to_owned())
                } else {
                    None
                };
                let _ = self.audit.record(&audit_event(
                    &execution_id,
                    &request,
                    policy_decision,
                    "rejected",
                    None,
                    None,
                ));
                return Err(error);
            }
        };

        // Fail closed before any side effect if the audit trail cannot record
        // the authorized execution attempt.
        self.audit
            .record(&audit_event(
                &execution_id,
                &request,
                Some("allowed".to_owned()),
                "started",
                None,
                None,
            ))
            .map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "audit sink unavailable; execution blocked before side effect",
                )
            })?;

        match self.runner.execute(&plan).await {
            Ok(result) => {
                let outcome = if result.success { "succeeded" } else { "failed" };
                let audit_recorded = self
                    .audit
                    .record(&audit_event(
                        &execution_id,
                        &request,
                        Some("allowed".to_owned()),
                        outcome,
                        result.exit_code,
                        Some(result.duration_ms),
                    ))
                    .is_ok();

                Ok(TaskExecutionOutcome {
                    execution_id,
                    result,
                    audit_recorded,
                })
            }
            Err(error) => {
                let _ = self.audit.record(&audit_event(
                    &execution_id,
                    &request,
                    Some("allowed".to_owned()),
                    "execution_error",
                    None,
                    None,
                ));
                Err(error)
            }
        }
    }
}

impl<P, R, A> TaskExecutionUseCase for TaskExecutionService<P, R, A>
where
    P: PlanPreparer + Sync,
    R: ExecutionRunner + Sync,
    A: AuditSink + Sync,
{
    fn execute(
        &self,
        request: TaskExecutionRequest,
    ) -> impl Future<Output = AppResult<TaskExecutionOutcome>> + Send {
        self.execute_inner(request)
    }
}

fn next_execution_id() -> String {
    let sequence = EXECUTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("exec-{millis}-{sequence}")
}

fn now_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

fn audit_event(
    execution_id: &str,
    request: &TaskRequest,
    policy_decision: Option<String>,
    outcome: &str,
    exit_code: Option<i32>,
    duration_ms: Option<u128>,
) -> AuditEvent {
    AuditEvent {
        request_id: execution_id.to_owned(),
        timestamp: now_timestamp(),
        target: request.target.0.clone(),
        operation: "run_task".to_owned(),
        task: Some(request.task.clone()),
        policy_decision,
        outcome: outcome.to_owned(),
        exit_code,
        duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use anyhow::{anyhow, Result};

    use super::*;
    use crate::domain::{CommandSpec, ExecutionOperation};

    struct StubPreparer {
        result: AppResult<ExecutionPlan>,
    }

    impl PlanPreparer for StubPreparer {
        fn prepare(&self, _request: &TaskRequest) -> AppResult<ExecutionPlan> {
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct StubRunner {
        called: AtomicBool,
    }

    impl ExecutionRunner for StubRunner {
        fn execute(
            &self,
            _plan: &ExecutionPlan,
        ) -> impl Future<Output = AppResult<ExecutionResult>> + Send {
            self.called.store(true, Ordering::Relaxed);
            async {
                Ok(ExecutionResult {
                    success: true,
                    exit_code: Some(0),
                    stdout: "ok".to_owned(),
                    stderr: String::new(),
                    duration_ms: 12,
                    stdout_truncated: false,
                    stderr_truncated: false,
                })
            }
        }
    }

    #[derive(Default)]
    struct MemoryAuditSink {
        events: Mutex<Vec<AuditEvent>>,
        fail: AtomicBool,
    }

    impl AuditSink for MemoryAuditSink {
        fn record(&self, event: &AuditEvent) -> Result<()> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(anyhow!("audit unavailable"));
            }
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    fn request() -> TaskExecutionRequest {
        TaskExecutionRequest {
            target: "test".to_owned(),
            task: "status".to_owned(),
            parameters: BTreeMap::new(),
        }
    }

    fn plan() -> ExecutionPlan {
        ExecutionPlan {
            target: TargetId("test".to_owned()),
            operation: ExecutionOperation::Command {
                command: CommandSpec {
                    program: "true".to_owned(),
                    args: vec![],
                },
            },
            timeout_seconds: 5,
        }
    }

    #[tokio::test]
    async fn orchestrates_plan_execute_and_audit() {
        let preparer = StubPreparer { result: Ok(plan()) };
        let runner = StubRunner::default();
        let audit = MemoryAuditSink::default();
        let service = TaskExecutionService::new(preparer, runner, audit);

        let outcome = service.execute(request()).await.unwrap();

        assert!(outcome.result.success);
        assert!(outcome.audit_recorded);
        assert!(service.runner.called.load(Ordering::Relaxed));
        let events = service.audit.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].outcome, "started");
        assert_eq!(events[1].outcome, "succeeded");
    }

    #[tokio::test]
    async fn planning_failure_never_calls_runner() {
        let preparer = StubPreparer {
            result: Err(AppError::new(ErrorCode::TaskNotAllowed, "denied")),
        };
        let runner = StubRunner::default();
        let audit = MemoryAuditSink::default();
        let service = TaskExecutionService::new(preparer, runner, audit);

        let error = service.execute(request()).await.unwrap_err();

        assert_eq!(error.code, ErrorCode::TaskNotAllowed);
        assert!(!service.runner.called.load(Ordering::Relaxed));
        let events = service.audit.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].policy_decision.as_deref(), Some("denied"));
    }

    #[tokio::test]
    async fn audit_failure_blocks_execution_before_side_effect() {
        let preparer = StubPreparer { result: Ok(plan()) };
        let runner = StubRunner::default();
        let audit = MemoryAuditSink::default();
        audit.fail.store(true, Ordering::Relaxed);
        let service = TaskExecutionService::new(preparer, runner, audit);

        let error = service.execute(request()).await.unwrap_err();

        assert_eq!(error.code, ErrorCode::Internal);
        assert!(!service.runner.called.load(Ordering::Relaxed));
    }
}
