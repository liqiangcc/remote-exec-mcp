pub mod ssh;

use crate::config::TargetTransportConfig;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub reachable: bool,
    pub remote_identity: Option<String>,
}

/// Connectivity only. A transport establishes a session; it does not own
/// task planning, authorization, command semantics, or file-transfer policy.
pub trait Transport: Send + Sync {
    type Session;

    fn connect(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = AppResult<Self::Session>> + Send;

    fn check(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = AppResult<ConnectionInfo>> + Send;
}
