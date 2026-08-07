use anyhow::Result;

use crate::config::TargetTransportConfig;

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub reachable: bool,
    pub remote_identity: Option<String>,
}

/// Connectivity only. A transport establishes a session; it does not own
/// task planning, authorization, command semantics, or file-transfer policy.
pub trait Transport {
    type Session;

    fn connect(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = Result<Self::Session>> + Send;

    fn check(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = Result<ConnectionInfo>> + Send;
}
