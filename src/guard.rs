use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::{AppError, AppResult, ErrorCode};

/// Fail-fast concurrency guard for remote operations.
///
/// The limiter is deliberately protocol-agnostic. MCP, CLI, or future adapters
/// all share the same runtime ceiling instead of each implementing their own
/// admission control.
#[derive(Clone)]
pub struct OperationLimiter {
    semaphore: Arc<Semaphore>,
}

impl OperationLimiter {
    pub fn new(max_concurrent_operations: usize) -> AppResult<Self> {
        if max_concurrent_operations == 0 {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "max_concurrent_operations must be greater than zero",
            ));
        }

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent_operations)),
        })
    }

    pub fn try_acquire(&self) -> AppResult<OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().map_err(|_| {
            AppError::new(
                ErrorCode::ConcurrencyLimitExceeded,
                "remote operation concurrency limit reached",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_capacity() {
        let error = OperationLimiter::new(0).err().unwrap();
        assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    }

    #[test]
    fn rejects_operations_above_capacity_without_queueing() {
        let limiter = OperationLimiter::new(1).unwrap();
        let permit = limiter.try_acquire().unwrap();

        let error = limiter.try_acquire().unwrap_err();
        assert_eq!(error.code, ErrorCode::ConcurrencyLimitExceeded);

        drop(permit);
        assert!(limiter.try_acquire().is_ok());
    }
}
