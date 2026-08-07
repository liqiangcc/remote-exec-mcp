use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};

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

impl AuditEvent {
    pub fn new(
        request_id: &str,
        target: &str,
        operation: &str,
        task: Option<&str>,
        outcome: &str,
        exit_code: Option<i32>,
        duration_ms: Option<u128>,
    ) -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            request_id: request_id.to_owned(),
            timestamp: millis.to_string(),
            target: target.to_owned(),
            operation: operation.to_owned(),
            task: task.map(str::to_owned),
            policy_decision: Some(if outcome == "denied" { "deny" } else { "allow" }.to_owned()),
            outcome: outcome.to_owned(),
            exit_code,
            duration_ms,
        }
    }
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AuditEvent) -> AppResult<()>;
}

pub struct JsonlAuditSink {
    file: Mutex<File>,
}

impl JsonlAuditSink {
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    ErrorCode::InvalidConfiguration,
                    format!(
                        "failed to create audit directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                AppError::new(
                    ErrorCode::InvalidConfiguration,
                    format!("failed to open audit log {}: {error}", path.display()),
                )
            })?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl AuditSink for JsonlAuditSink {
    fn record(&self, event: &AuditEvent) -> AppResult<()> {
        let serialized = serde_json::to_string(event).map_err(|error| {
            AppError::new(
                ErrorCode::Internal,
                format!("failed to serialize audit event: {error}"),
            )
        })?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "audit log mutex was poisoned"))?;
        writeln!(file, "{serialized}").map_err(|error| {
            AppError::new(
                ErrorCode::Internal,
                format!("failed to persist audit event: {error}"),
            )
        })?;
        file.flush().map_err(|error| {
            AppError::new(
                ErrorCode::Internal,
                format!("failed to flush audit event: {error}"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_sink_persists_machine_readable_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::open(&path).unwrap();
        sink.record(&AuditEvent::new(
            "req-1",
            "test",
            "run_task",
            Some("status"),
            "success",
            Some(0),
            Some(12),
        ))
        .unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        let value: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(value["request_id"], "req-1");
        assert_eq!(value["target"], "test");
        assert_eq!(value["outcome"], "success");
    }
}
