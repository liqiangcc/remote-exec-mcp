pub mod task_execution_service;

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::Value;

use crate::config::Config;
use crate::domain::{
    CommandSpec, ExecutionOperation, ExecutionPlan, ExecutionTemplate, TaskRequest, TaskSpec,
};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::policy::{DefaultPolicyEngine, PolicyEngine};
use crate::validation::{DefaultRequestValidator, RequestValidator};

/// Converts validated, authorized intent into an executable plan.
/// Concrete executors and transports are deliberately outside this boundary.
pub trait TaskPlanner {
    fn plan(&self, request: &TaskRequest, task: &TaskSpec) -> AppResult<ExecutionPlan>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTaskPlanner;

impl TaskPlanner for DefaultTaskPlanner {
    fn plan(&self, request: &TaskRequest, task: &TaskSpec) -> AppResult<ExecutionPlan> {
        let operation = match &task.execution {
            ExecutionTemplate::Command { program, args } => {
                if program.trim().is_empty() {
                    return Err(AppError::new(
                        ErrorCode::InvalidTaskDefinition,
                        "task command program must not be empty",
                    ));
                }
                if program.contains("{{") || program.contains("}}") {
                    return Err(AppError::new(
                        ErrorCode::InvalidTaskDefinition,
                        "task command program must be static",
                    ));
                }

                let args = args
                    .iter()
                    .map(|arg| render_template(arg, &request.parameters))
                    .collect::<AppResult<Vec<_>>>()?;

                ExecutionOperation::Command {
                    command: CommandSpec {
                        program: program.clone(),
                        args,
                    },
                }
            }
        };

        Ok(ExecutionPlan {
            target: request.target.clone(),
            operation,
            timeout_seconds: task.definition.timeout_seconds,
        })
    }
}

/// Orchestrates the application-level planning use case while preserving
/// separation between policy, validation and plan construction.
pub struct PlanningService<'a, V, E, P> {
    config: &'a Config,
    validator: V,
    policy: E,
    planner: P,
}

impl<'a> PlanningService<'a, DefaultRequestValidator, DefaultPolicyEngine, DefaultTaskPlanner> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            validator: DefaultRequestValidator,
            policy: DefaultPolicyEngine,
            planner: DefaultTaskPlanner,
        }
    }
}

impl<'a, V, E, P> PlanningService<'a, V, E, P> {
    pub fn with_components(config: &'a Config, validator: V, policy: E, planner: P) -> Self {
        Self {
            config,
            validator,
            policy,
            planner,
        }
    }
}

impl<'a, V, E, P> PlanningService<'a, V, E, P>
where
    V: RequestValidator,
    E: PolicyEngine,
    P: TaskPlanner,
{
    pub fn prepare(&self, request: &TaskRequest) -> AppResult<ExecutionPlan> {
        let target_policy = self.config.target_policy(&request.target).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTarget,
                format!("unknown target: {}", request.target.0),
            )
        })?;

        // Authorize the intent before resolving task implementation details.
        self.policy.authorize(request, &target_policy)?;

        let task = self.config.task_spec(&request.task).ok_or_else(|| {
            AppError::new(
                ErrorCode::UnknownTask,
                format!("unknown task: {}", request.task),
            )
        })?;

        self.validator.validate(request, &task.definition)?;
        self.planner.plan(request, &task)
    }
}

fn render_template(template: &str, parameters: &BTreeMap<String, Value>) -> AppResult<String> {
    let placeholder =
        Regex::new(r"\{\{([a-zA-Z0-9_.-]+)\}\}").expect("static placeholder regex must be valid");
    let mut rendered = String::with_capacity(template.len());
    let mut last = 0;

    for captures in placeholder.captures_iter(template) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let Some(name_match) = captures.get(1) else {
            continue;
        };

        rendered.push_str(&template[last..full_match.start()]);
        let name = name_match.as_str();
        let value = parameters.get(name).ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidTaskDefinition,
                format!("missing value for task placeholder: {name}"),
            )
        })?;
        rendered.push_str(&render_parameter(value)?);
        last = full_match.end();
    }

    rendered.push_str(&template[last..]);

    if rendered.contains("{{") || rendered.contains("}}") {
        return Err(AppError::new(
            ErrorCode::InvalidTaskDefinition,
            "invalid or unresolved task placeholder",
        ));
    }

    Ok(rendered)
}

fn render_parameter(value: &Value) -> AppResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(AppError::new(
            ErrorCode::InvalidParameter,
            "task placeholders only support scalar parameter values",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::domain::TargetId;

    fn config() -> Config {
        serde_yaml::from_str(
            r#"
targets:
  test:
    transport:
      type: ssh
      host: 127.0.0.1
      user: deploy
      auth:
        type: key
        secret_ref: ssh-key:test
    policy:
      allowed_tasks: [service-status]
tasks:
  service-status:
    parameters:
      service:
        type: string
        pattern: "^[a-zA-Z0-9_.@-]+$"
        required: true
    execution:
      type: command
      program: systemctl
      args: ["status", "{{service}}", "--no-pager"]
    timeout_seconds: 15
"#,
        )
        .unwrap()
    }

    fn request(service: &str) -> TaskRequest {
        TaskRequest {
            target: TargetId("test".to_owned()),
            task: "service-status".to_owned(),
            parameters: BTreeMap::from([("service".to_owned(), json!(service))]),
        }
    }

    #[test]
    fn prepares_typed_authorized_command_plan() {
        let config = config();
        let plan = PlanningService::new(&config)
            .prepare(&request("demo.service"))
            .unwrap();

        assert_eq!(plan.timeout_seconds, 15);
        match plan.operation {
            ExecutionOperation::Command { command } => {
                assert_eq!(command.program, "systemctl");
                assert_eq!(
                    command.args,
                    vec![
                        "status".to_owned(),
                        "demo.service".to_owned(),
                        "--no-pager".to_owned(),
                    ]
                );
            }
            _ => panic!("expected command plan"),
        }
    }

    #[test]
    fn rejects_parameter_before_planning_with_stable_code() {
        let config = config();
        let error = PlanningService::new(&config)
            .prepare(&request("demo; shutdown -h now"))
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidParameter);
    }

    #[test]
    fn rejects_task_not_allowed_by_target_policy() {
        let mut config = config();
        config
            .targets
            .get_mut("test")
            .unwrap()
            .policy
            .allowed_tasks
            .clear();

        let error = PlanningService::new(&config)
            .prepare(&request("demo.service"))
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::TaskNotAllowed);
    }

    #[test]
    fn returns_unknown_target_code() {
        let config = config();
        let mut request = request("demo.service");
        request.target = TargetId("missing".to_owned());

        let error = PlanningService::new(&config).prepare(&request).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnknownTarget);
    }

    #[test]
    fn planner_keeps_program_static_and_parameters_as_arguments() {
        let task = TaskSpec {
            definition: crate::domain::TaskDefinition {
                name: "echo".to_owned(),
                description: None,
                parameters: BTreeMap::new(),
                timeout_seconds: 5,
            },
            execution: ExecutionTemplate::Command {
                program: "{{program}}".to_owned(),
                args: Vec::new(),
            },
        };
        let request = TaskRequest {
            target: TargetId("test".to_owned()),
            task: "echo".to_owned(),
            parameters: BTreeMap::from([("program".to_owned(), json!("sh"))]),
        };

        let error = DefaultTaskPlanner.plan(&request, &task).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidTaskDefinition);
    }
}
