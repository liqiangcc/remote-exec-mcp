use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
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
    pub error_code: Option<String>,
    pub exit_code: Option<i32>,
    pub bytes_transferred: Option<u64>,
    pub duration_ms: Option<u128>,
}

/// Persistence boundary for security-relevant operation audit events.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AuditEvent) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _event: &AuditEvent) -> Result<()> {
        Ok(())
    }
}

/// Append-only JSON Lines audit sink.
///
/// Each event is serialized independently and flushed before returning. A
/// process-local mutex prevents concurrent records from interleaving bytes.
pub struct JsonlAuditSink {
    path: PathBuf,
    file: Mutex<File>,
}

impl JsonlAuditSink {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create audit directory {}", parent.display())
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open audit log {}", path.display()))?;

        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AuditSink for JsonlAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow!("audit log mutex poisoned"))?;
        serde_json::to_writer(&mut *file, event).context("failed to serialize audit event")?;
        file.write_all(b"\n")
            .context("failed to terminate audit record")?;
        file.flush().context("failed to flush audit record")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(outcome: &str) -> AuditEvent {
        AuditEvent {
            request_id: "req-1".to_owned(),
            timestamp: "123".to_owned(),
            target: "test".to_owned(),
            operation: "run_task".to_owned(),
            task: Some("status".to_owned()),
            policy_decision: Some("allowed".to_owned()),
            outcome: outcome.to_owned(),
            error_code: None,
            exit_code: Some(0),
            bytes_transferred: None,
            duration_ms: Some(5),
        }
    }

    #[test]
    fn appends_one_json_object_per_line() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit/events.jsonl");
        let sink = JsonlAuditSink::open(&path).unwrap();

        sink.record(&event("started")).unwrap();
        sink.record(&event("succeeded")).unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        let second: AuditEvent = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first.outcome, "started");
        assert_eq!(second.outcome, "succeeded");
    }
}
