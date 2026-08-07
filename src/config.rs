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
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub targets: BTreeMap<String, TargetConfig>,
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "default_transfer_timeout")]
    pub transfer_timeout_seconds: u64,
    #[serde(default = "default_audit_path")]
    pub audit_path: String,
    #[serde(default)]
    pub allowed_local_upload_roots: Vec<String>,
    #[serde(default)]
    pub allowed_local_download_roots: Vec<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrency: default_max_concurrency(),
            transfer_timeout_seconds: default_transfer_timeout(),
            audit_path: default_audit_path(),
            allowed_local_upload_roots: Vec::new(),
            allowed_local_download_roots: Vec::new(),
        }
    }
}

fn default_max_concurrency() -> usize {
    4
}

fn default_transfer_timeout() -> u64 {
    60
}

fn default_audit_path() -> String {
    ".remote-exec-mcp/audit.jsonl".to_owned()
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
        known_hosts_path: Option<String>,
        #[serde(default = "default_connect_timeout")]
        connect_timeout_seconds: u64,
    },
}

fn default_port() -> u16 {
    22
}

fn default_connect_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    Key { secret_ref: String },
    Password { secret_ref: String },
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

    pub fn target_transport(&self, target: &TargetId) -> Option<&TargetTransportConfig> {
        self.targets.get(&target.0).map(|target| &target.transport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_config_into_domain_task_policy_and_runtime_defaults() {
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
        secret_ref: env:TEST_SSH_KEY
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
        assert_eq!(config.runtime.max_concurrency, 4);
        assert_eq!(config.runtime.transfer_timeout_seconds, 60);
        assert!(config
            .target_transport(&TargetId("test".to_owned()))
            .is_some());
    }

    #[test]
    fn parses_password_auth_as_secret_reference() {
        let config: Config = serde_yaml::from_str(
            r#"
targets:
  test:
    transport:
      type: ssh
      host: 127.0.0.1
      user: deploy
      auth:
        type: password
        secret_ref: env:TEST_SSH_PASSWORD
"#,
        )
        .unwrap();

        let TargetTransportConfig::Ssh { auth, .. } = config
            .target_transport(&TargetId("test".to_owned()))
            .unwrap();
        assert!(matches!(auth, AuthConfig::Password { secret_ref } if secret_ref == "env:TEST_SSH_PASSWORD"));
    }
}
