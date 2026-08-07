use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRequest {
    pub target: TargetId,
    pub task: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterDefinition>,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

fn default_timeout_seconds() -> u64 {
    30
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterDefinition {
    #[serde(rename = "type")]
    pub kind: ParameterType,
    pub pattern: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterType {
    String,
    Integer,
    Boolean,
}

/// A task remains an intent-level definition. The template is resolved into an
/// ExecutionPlan only after authorization and parameter validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub definition: TaskDefinition,
    pub execution: ExecutionTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionTemplate {
    Command { program: String, args: Vec<String> },
}

/// Target policy is a domain value object. Security logic does not depend on
/// the YAML/config representation used by infrastructure code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPolicy {
    pub allowed_tasks: BTreeSet<String>,
    /// Remote destinations allowed for uploads.
    pub allowed_upload_roots: Vec<String>,
    /// Remote sources allowed for downloads.
    pub allowed_download_roots: Vec<String>,
    /// Local MCP-host sources allowed for uploads.
    pub allowed_local_upload_roots: Vec<String>,
    /// Local MCP-host destinations allowed for downloads.
    pub allowed_local_download_roots: Vec<String>,
    pub max_transfer_bytes: Option<u64>,
    pub transfer_timeout_seconds: u64,
}

impl Default for TargetPolicy {
    fn default() -> Self {
        Self {
            allowed_tasks: BTreeSet::new(),
            allowed_upload_roots: Vec::new(),
            allowed_download_roots: Vec::new(),
            allowed_local_upload_roots: Vec::new(),
            allowed_local_download_roots: Vec::new(),
            max_transfer_bytes: None,
            transfer_timeout_seconds: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub target: TargetId,
    pub operation: ExecutionOperation,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionOperation {
    Command { command: CommandSpec },
    Upload { transfer: TransferSpec },
    Download { transfer: TransferSpec },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSpec {
    pub source: String,
    pub destination: String,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}
