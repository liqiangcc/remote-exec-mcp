use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef(pub String);

/// Resolves secret references without exposing secret storage to MCP/domain code.
pub trait SecretProvider {
    fn resolve(&self, secret: &SecretRef) -> Result<String>;
}
