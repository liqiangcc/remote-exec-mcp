use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::domain::{
    ExecutionTemplate, ParameterDefinition, TargetId, TargetPolicy, TaskDefinition, TaskSpec,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub targets: BTreeMap<String, TargetConfig>,
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    pub transport: TargetTransportConfig,
    #[serde(default)]
    pub policy: TargetPolicyConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TargetTransportConfig {
    Ssh {
        host: String,
        #[serde(default = "default_port")]
        port: u16,
        user: String,
        auth: AuthConfig,
        #[serde(default)]
        host_key_policy: HostKeyPolicy,
    },
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    Key { secret_ref: String },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostKeyPolicy {
    #[default]
    Strict,
    AcceptNew,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TargetPolicyConfig {
    #[serde(default)]
    pub allowed_tasks: Vec<String>,
    #[serde(default)]
    pub allowed_upload_roots: Vec<String>,
    #[serde(default)]
    pub allowed_download_roots: Vec<String>,
    pub max_transfer_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterDefinition>,
    pub execution: TaskExecutionConfig,
    #[serde(default = "default_task_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskExecutionConfig {
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

fn default_task_timeout() -> u64 {
    30
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse config {}", path.display()))
    }

    pub fn task_definition(&self, name: &str) -> Option<TaskDefinition> {
        self.tasks.get(name).map(|task| TaskDefinition {
            name: name.to_owned(),
            description: task.description.clone(),
            parameters: task.parameters.clone(),
            timeout_seconds: task.timeout_seconds,
        })
    }

    pub fn task_spec(&self, name: &str) -> Option<TaskSpec> {
        self.tasks.get(name).map(|task| {
            let execution = match &task.execution {
                TaskExecutionConfig::Command { program, args } => ExecutionTemplate::Command {
                    program: program.clone(),
                    args: args.clone(),
                },
            };

            TaskSpec {
                definition: TaskDefinition {
                    name: name.to_owned(),
                    description: task.description.clone(),
                    parameters: task.parameters.clone(),
                    timeout_seconds: task.timeout_seconds,
                },
                execution,
            }
        })
    }

    pub fn target_policy(&self, target: &TargetId) -> Option<TargetPolicy> {
        self.targets.get(&target.0).map(|target| TargetPolicy {
            allowed_tasks: target.policy.allowed_tasks.iter().cloned().collect(),
            allowed_upload_roots: target.policy.allowed_upload_roots.clone(),
            allowed_download_roots: target.policy.allowed_download_roots.clone(),
            max_transfer_bytes: target.policy.max_transfer_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_config_into_domain_task_and_policy() {
        let config: Config = serde_yaml::from_str(
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
      allowed_tasks: [whoami]
tasks:
  whoami:
    execution:
      type: command
      program: whoami
"#,
        )
        .unwrap();

        let task = config.task_spec("whoami").unwrap();
        assert_eq!(task.definition.name, "whoami");

        let policy = config.target_policy(&TargetId("test".to_owned())).unwrap();
        assert!(policy.allowed_tasks.contains("whoami"));
    }
}
