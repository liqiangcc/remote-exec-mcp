use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{AppError, AppResult, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef(pub String);

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// Resolves secret references without exposing secret storage to MCP/domain code.
pub trait SecretProvider: Send + Sync {
    fn resolve(&self, secret: &SecretRef) -> AppResult<SecretValue>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EnvSecretProvider;

impl SecretProvider for EnvSecretProvider {
    fn resolve(&self, secret: &SecretRef) -> AppResult<SecretValue> {
        let variable = secret.0.strip_prefix("env:").ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("unsupported secret reference scheme: {}", secret.0),
            )
        })?;

        if variable.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "environment secret reference must include a variable name",
            ));
        }

        std::env::var(variable).map(SecretValue::new).map_err(|_| {
            AppError::new(
                ErrorCode::SecretNotFound,
                format!("secret environment variable is not available: {variable}"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let secret = SecretValue::new("super-secret");
        assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
    }

    #[test]
    fn rejects_unknown_secret_reference_scheme() {
        let error = EnvSecretProvider
            .resolve(&SecretRef("vault:prod/key".to_owned()))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    }
}
