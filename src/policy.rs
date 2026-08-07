use std::path::{Component, Path};

use anyhow::{bail, Result};

use crate::domain::{TargetPolicy, TaskRequest};

pub trait PolicyEngine {
    fn authorize(&self, request: &TaskRequest, policy: &TargetPolicy) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn authorize(&self, request: &TaskRequest, policy: &TargetPolicy) -> Result<()> {
        ensure_task_allowed(policy, &request.task)
    }
}

pub fn ensure_task_allowed(policy: &TargetPolicy, task: &str) -> Result<()> {
    if policy.allowed_tasks.contains(task) {
        return Ok(());
    }
    bail!("task is not allowed for this target")
}

pub fn ensure_remote_path_allowed(path: &str, roots: &[String]) -> Result<()> {
    let candidate = Path::new(path);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("parent path traversal is not allowed")
    }
    if !candidate.is_absolute() {
        bail!("remote path must be absolute")
    }

    let allowed = roots
        .iter()
        .any(|root| candidate.starts_with(Path::new(root)));
    if !allowed {
        bail!("remote path is outside configured roots")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::TargetId;

    #[test]
    fn allows_configured_task() {
        let policy = TargetPolicy {
            allowed_tasks: ["whoami".to_owned()].into_iter().collect(),
            ..TargetPolicy::default()
        };
        let request = TaskRequest {
            target: TargetId("test".to_owned()),
            task: "whoami".to_owned(),
            parameters: BTreeMap::new(),
        };

        assert!(DefaultPolicyEngine.authorize(&request, &policy).is_ok());
    }

    #[test]
    fn denies_unconfigured_task() {
        let policy = TargetPolicy::default();
        let request = TaskRequest {
            target: TargetId("test".to_owned()),
            task: "restart".to_owned(),
            parameters: BTreeMap::new(),
        };

        assert!(DefaultPolicyEngine.authorize(&request, &policy).is_err());
    }

    #[test]
    fn rejects_parent_traversal() {
        let roots = vec!["/opt/apps".to_string()];
        assert!(ensure_remote_path_allowed("/opt/apps/../etc/passwd", &roots).is_err());
    }

    #[test]
    fn accepts_path_under_root() {
        let roots = vec!["/opt/apps".to_string()];
        assert!(ensure_remote_path_allowed("/opt/apps/demo/app.jar", &roots).is_ok());
    }
}
