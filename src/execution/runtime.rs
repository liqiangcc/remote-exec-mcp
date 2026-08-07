use std::future::Future;

use crate::application::task_execution_service::ExecutionRunner;
use crate::config::Config;
use crate::domain::{ExecutionOperation, ExecutionPlan, ExecutionResult};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::ssh::SshCommandExecutor;
use crate::execution::{CommandExecutor, ExecutionLimits};
use crate::secret::SecretProvider;
use crate::transport::ssh::SshTransport;
use crate::transport::Transport;

/// Infrastructure adapter that turns an application `ExecutionPlan` into an
/// authenticated SSH session plus the appropriate concrete executor.
///
/// The application layer depends only on `ExecutionRunner`; SSH/config/secret
/// concerns remain on this side of the port.
pub struct ConfiguredSshExecutionRunner<'a, P> {
    config: &'a Config,
    transport: SshTransport<P>,
    command_executor: SshCommandExecutor,
}

impl<'a, P> ConfiguredSshExecutionRunner<'a, P> {
    pub fn new(config: &'a Config, secrets: P) -> Self {
        Self {
            config,
            transport: SshTransport::new(secrets),
            command_executor: SshCommandExecutor,
        }
    }
}

impl<P> ConfiguredSshExecutionRunner<'_, P>
where
    P: SecretProvider,
{
    async fn execute_plan(&self, plan: &ExecutionPlan) -> AppResult<ExecutionResult> {
        let target = self.config.targets.get(&plan.target.0).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", plan.target.0),
            )
        })?;

        let mut session = self.transport.connect(&target.transport).await?;

        match &plan.operation {
            ExecutionOperation::Command { command } => {
                self.command_executor
                    .execute(
                        &mut session,
                        command,
                        ExecutionLimits::new(plan.timeout_seconds),
                    )
                    .await
            }
            ExecutionOperation::Upload { .. } | ExecutionOperation::Download { .. } => Err(
                AppError::new(
                    ErrorCode::InvalidTaskDefinition,
                    "run_task execution runner received a non-command execution plan",
                ),
            ),
        }
    }
}

impl<P> ExecutionRunner for ConfiguredSshExecutionRunner<'_, P>
where
    P: SecretProvider + Sync,
{
    fn execute(
        &self,
        plan: &ExecutionPlan,
    ) -> impl Future<Output = AppResult<ExecutionResult>> + Send {
        self.execute_plan(plan)
    }
}
