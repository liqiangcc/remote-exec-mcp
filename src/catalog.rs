use crate::config::Config;
use crate::domain::{TargetId, TaskDefinition};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::policy::ensure_task_allowed;

/// Read-only application catalog exposed to protocol adapters.
///
/// It intentionally returns only tasks allowed by each target policy, so an
/// adapter cannot accidentally advertise globally-defined but unauthorized
/// operations for a target.
pub trait Catalog {
    fn list_targets(&self) -> Vec<TargetId>;
    fn list_tasks(&self, target: &TargetId) -> AppResult<Vec<TaskDefinition>>;
    fn task(&self, target: &TargetId, task: &str) -> AppResult<TaskDefinition>;
}

pub struct ConfigCatalog<'a> {
    config: &'a Config,
}

impl<'a> ConfigCatalog<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }
}

impl Catalog for ConfigCatalog<'_> {
    fn list_targets(&self) -> Vec<TargetId> {
        self.config.targets.keys().cloned().map(TargetId).collect()
    }

    fn list_tasks(&self, target: &TargetId) -> AppResult<Vec<TaskDefinition>> {
        let policy = self.config.target_policy(target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })?;

        policy
            .allowed_tasks
            .iter()
            .map(|name| {
                self.config.task_definition(name).ok_or_else(|| {
                    AppError::new(
                        ErrorCode::InvalidConfiguration,
                        format!(
                            "target {} allows task {name}, but the task is not defined",
                            target.0
                        ),
                    )
                })
            })
            .collect()
    }

    fn task(&self, target: &TargetId, task: &str) -> AppResult<TaskDefinition> {
        let policy = self.config.target_policy(target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", target.0),
            )
        })?;

        ensure_task_allowed(&policy, task)?;

        self.config.task_definition(task).ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!(
                    "target {} allows task {task}, but the task is not defined",
                    target.0
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        serde_yaml::from_str(
            r#"
targets:
  alpha:
    transport:
      type: ssh
      host: 127.0.0.1
      user: deploy
      auth:
        type: key
        secret_ref: ssh-key:alpha
    policy:
      allowed_tasks: [status]
  beta:
    transport:
      type: ssh
      host: 127.0.0.2
      user: deploy
      auth:
        type: key
        secret_ref: ssh-key:beta
    policy:
      allowed_tasks: [status, restart]
tasks:
  status:
    description: Show status
    execution:
      type: command
      program: "true"
  restart:
    description: Restart service
    execution:
      type: command
      program: "true"
  hidden:
    description: Globally defined but not allowed anywhere
    execution:
      type: command
      program: "true"
"#,
        )
        .unwrap()
    }

    #[test]
    fn lists_targets_in_stable_order() {
        let config = config();
        let catalog = ConfigCatalog::new(&config);

        assert_eq!(
            catalog.list_targets(),
            vec![TargetId("alpha".to_owned()), TargetId("beta".to_owned())]
        );
    }

    #[test]
    fn lists_only_tasks_allowed_for_target() {
        let config = config();
        let catalog = ConfigCatalog::new(&config);

        let tasks = catalog.list_tasks(&TargetId("alpha".to_owned())).unwrap();
        let names: Vec<_> = tasks.into_iter().map(|task| task.name).collect();

        assert_eq!(names, vec!["status"]);
    }

    #[test]
    fn does_not_expose_disallowed_global_task() {
        let config = config();
        let catalog = ConfigCatalog::new(&config);

        let error = catalog
            .task(&TargetId("alpha".to_owned()), "hidden")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskNotAllowed);
    }

    #[test]
    fn reports_unknown_target_with_stable_code() {
        let config = config();
        let catalog = ConfigCatalog::new(&config);

        let error = catalog
            .list_tasks(&TargetId("missing".to_owned()))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnknownTarget);
    }

    #[test]
    fn reports_policy_reference_to_missing_task_as_invalid_configuration() {
        let mut config = config();
        config
            .targets
            .get_mut("alpha")
            .unwrap()
            .policy
            .allowed_tasks
            .push("missing-task".to_owned());
        let catalog = ConfigCatalog::new(&config);

        let error = catalog
            .list_tasks(&TargetId("alpha".to_owned()))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    }
}
