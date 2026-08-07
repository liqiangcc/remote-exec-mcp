use std::path::{Component, Path};

use crate::domain::{TargetPolicy, TaskRequest};
use crate::error::{AppError, AppResult, ErrorCode};

pub trait PolicyEngine {
    fn authorize(&self, request: &TaskRequest, policy: &TargetPolicy) -> AppResult<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn authorize(&self, request: &TaskRequest, policy: &TargetPolicy) -> AppResult<()> {
        ensure_task_allowed(policy, &request.task)
    }
}

pub fn ensure_task_allowed(policy: &TargetPolicy, task: &str) -> AppResult<()> {
    if policy.allowed_tasks.contains(task) {
        return Ok(());
    }

    Err(AppError::new(
        ErrorCode::TaskNotAllowed,
        "task is not allowed for this target",
    ))
}

pub fn ensure_remote_path_allowed(path: &str, roots: &[String]) -> AppResult<()> {
    let candidate = Path::new(path);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AppError::new(
            ErrorCode::InvalidRequest,
            "parent path traversal is not allowed",
        ));
    }
    if !candidate.is_absolute() {
        return Err(AppError::new(
            ErrorCode::InvalidRequest,
            "remote path must be absolute",
        ));
    }

    let allowed = roots
        .iter()
        .any(|root| candidate.starts_with(Path::new(root)));
    if !allowed {
        return Err(AppError::new(
            ErrorCode::InvalidRequest,
            "remote path is outside configured roots",
        ));
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
    fn denies_unconfigured_task_with_stable_code() {
        let policy = TargetPolicy::default();
        let request = TaskRequest {
            target: TargetId("test".to_owned()),
            task: "restart".to_owned(),
            parameters: BTreeMap::new(),
        };

        let error = DefaultPolicyEngine.authorize(&request, &policy).unwrap_err();
        assert_eq!(error.code, ErrorCode::TaskNotAllowed);
    }

    #[test]
    fn rejects_parent_traversal() {
        let roots = vec!["/opt/apps".to_string()];
        let error = ensure_remote_path_allowed("/opt/apps/../etc/passwd", &roots).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn accepts_path_under_root() {
        let roots = vec!["/opt/apps".to_string()];
        assert!(ensure_remote_path_allowed("/opt/apps/demo/app.jar", &roots).is_ok());
    }
}
