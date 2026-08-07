use std::path::{Component, Path};

use anyhow::{bail, Result};

use crate::config::TargetPolicyConfig;

pub fn ensure_task_allowed(policy: &TargetPolicyConfig, task: &str) -> Result<()> {
    if policy.allowed_tasks.iter().any(|allowed| allowed == task) {
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
    use super::*;

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
