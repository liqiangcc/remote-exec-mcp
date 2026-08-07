use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub request_id: String,
    pub timestamp: String,
    pub target: String,
    pub operation: String,
    pub task: Option<String>,
    pub policy_decision: Option<String>,
    pub outcome: String,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
}

pub trait AuditSink {
    fn record(&self, event: &AuditEvent) -> Result<()>;
}
