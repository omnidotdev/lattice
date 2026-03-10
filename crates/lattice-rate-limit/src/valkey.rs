use deadpool_redis::{Config, Pool, Runtime, redis::cmd};

use crate::{RateLimitError, RateLimitResult, RateLimiter};

/// Rate limiter implementation backed by Valkey (Redis-compatible).
///
/// Uses fixed-window counters for rate limiting.
pub struct ValkeyRateLimiter {
    pool: Pool,
    prefix: String,
}

impl ValkeyRateLimiter {
    /// Create a new Valkey-backed rate limiter.
    ///
    /// # Errors
    ///
    /// Returns `RateLimitError::Backend` if the connection pool cannot be created.
    pub fn new(url: &str, prefix: String) -> Result<Self, RateLimitError> {
        let config = Config::from_url(url);

        let pool = config
            .create_pool(Some(Runtime::Tokio1))
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        Ok(Self { pool, prefix })
    }

    /// Build a rate limit key with the configured prefix.
    fn rate_limit_key(&self, key: &str, window_seconds: u64) -> String {
        format!("{}rl:{key}:{window_seconds}", self.prefix)
    }
}

impl RateLimiter for ValkeyRateLimiter {
    async fn check(
        &self,
        key: &str,
        max_requests: u64,
        window_seconds: u64,
    ) -> Result<RateLimitResult, RateLimitError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        let rl_key = self.rate_limit_key(key, window_seconds);

        // Increment the counter for this window.
        let count: u64 = cmd("INCR")
            .arg(&[&rl_key])
            .query_async(&mut *conn)
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        // Set expiry on first request in the window.
        if count == 1 {
            cmd("EXPIRE")
                .arg(&rl_key)
                .arg(window_seconds)
                .query_async::<()>(&mut *conn)
                .await
                .map_err(|e| RateLimitError::Backend(e.to_string()))?;
        }

        // Get time until the window resets.
        let ttl: i64 = cmd("TTL")
            .arg(&[&rl_key])
            .query_async(&mut *conn)
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        let reset_after = u64::try_from(ttl.max(0)).unwrap_or(window_seconds);
        let allowed = count <= max_requests;
        let remaining = if allowed { max_requests - count } else { 0 };

        tracing::debug!(
            key,
            count,
            max_requests,
            allowed,
            remaining,
            reset_after,
            "rate limit check"
        );

        Ok(RateLimitResult {
            allowed,
            limit: max_requests,
            remaining,
            reset_after,
        })
    }
}
