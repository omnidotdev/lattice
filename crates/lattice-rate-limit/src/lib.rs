mod valkey;

pub use valkey::ValkeyRateLimiter;

/// Errors that can occur during rate limiting operations.
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    /// An error from the underlying rate limit backend.
    #[error("rate limit backend error: {0}")]
    Backend(String),
}

/// Result of a rate limit check.
#[derive(Debug)]
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Maximum number of requests permitted in the window.
    pub limit: u64,
    /// Remaining requests in the current window.
    pub remaining: u64,
    /// Seconds until the current window resets.
    pub reset_after: u64,
}

/// Trait for async rate limiting.
pub trait RateLimiter: Send + Sync {
    /// Check whether a request identified by `key` is within the rate limit.
    ///
    /// # Errors
    ///
    /// Returns `RateLimitError::Backend` if the underlying store is unreachable.
    fn check(
        &self,
        key: &str,
        max_requests: u64,
        window_seconds: u64,
    ) -> impl Future<Output = Result<RateLimitResult, RateLimitError>> + Send;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// Per-key window state for the in-memory rate limiter.
    struct WindowState {
        count: u64,
        window_start: u64,
    }

    /// In-memory rate limiter for testing purposes.
    struct InMemoryRateLimiter {
        store: Mutex<HashMap<String, WindowState>>,
    }

    impl InMemoryRateLimiter {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }

        /// Return a simple monotonic "now" based on wall clock seconds.
        fn now_secs() -> u64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_secs()
        }
    }

    impl RateLimiter for InMemoryRateLimiter {
        async fn check(
            &self,
            key: &str,
            max_requests: u64,
            window_seconds: u64,
        ) -> Result<RateLimitResult, RateLimitError> {
            let mut store = self.store.lock().map_err(|e| RateLimitError::Backend(e.to_string()))?;

            let now = Self::now_secs();
            let window_start = now - (now % window_seconds);

            let state = store
                .entry(key.to_owned())
                .or_insert_with(|| WindowState { count: 0, window_start });

            // Reset if the window has rolled over.
            if state.window_start != window_start {
                state.count = 0;
                state.window_start = window_start;
            }

            state.count += 1;

            let allowed = state.count <= max_requests;
            let remaining = if allowed { max_requests - state.count } else { 0 };
            let reset_after = window_seconds - (now - window_start);

            Ok(RateLimitResult {
                allowed,
                limit: max_requests,
                remaining,
                reset_after,
            })
        }
    }

    #[tokio::test]
    async fn allows_within_limit() {
        let limiter = InMemoryRateLimiter::new();

        let result = limiter.check("user:1", 5, 60).await.unwrap();
        assert!(result.allowed);
        assert_eq!(result.limit, 5);
        assert_eq!(result.remaining, 4);
    }

    #[tokio::test]
    async fn rejects_over_limit() {
        let limiter = InMemoryRateLimiter::new();

        for _ in 0..5 {
            let result = limiter.check("user:1", 5, 60).await.unwrap();
            assert!(result.allowed);
        }

        let result = limiter.check("user:1", 5, 60).await.unwrap();
        assert!(!result.allowed);
        assert_eq!(result.remaining, 0);
    }

    #[tokio::test]
    async fn different_keys_independent() {
        let limiter = InMemoryRateLimiter::new();

        for _ in 0..5 {
            limiter.check("user:1", 5, 60).await.unwrap();
        }

        let result = limiter.check("user:2", 5, 60).await.unwrap();
        assert!(result.allowed);
        assert_eq!(result.remaining, 4);
    }
}
