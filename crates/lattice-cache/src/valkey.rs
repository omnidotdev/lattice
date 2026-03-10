use deadpool_redis::{Config, Pool, Runtime, redis::cmd};

use crate::{CacheError, EntityCache};

/// Cache implementation backed by Valkey (Redis-compatible).
pub struct ValkeyCache {
    pool: Pool,
    prefix: String,
}

impl ValkeyCache {
    /// Create a new Valkey-backed cache.
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Backend` if the connection pool cannot be created.
    pub fn new(url: &str, prefix: String) -> Result<Self, CacheError> {
        let config = Config::from_url(url);

        let pool = config
            .create_pool(Some(Runtime::Tokio1))
            .map_err(|e| CacheError::Backend(e.to_string()))?;

        Ok(Self { pool, prefix })
    }

    /// Build a prefixed key.
    fn prefixed_key(&self, key: &str) -> String {
        format!("{}:{key}", self.prefix)
    }
}

impl EntityCache for ValkeyCache {
    async fn get(&self, key: &str) -> Result<Option<String>, CacheError> {
        let mut conn = self.pool.get().await.map_err(|e| CacheError::Backend(e.to_string()))?;

        let result: Option<String> = cmd("GET")
            .arg(&[self.prefixed_key(key)])
            .query_async(&mut *conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;

        Ok(result)
    }

    async fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), CacheError> {
        let mut conn = self.pool.get().await.map_err(|e| CacheError::Backend(e.to_string()))?;

        cmd("SET")
            .arg(&[self.prefixed_key(key).as_str(), value])
            .arg("EX")
            .arg(ttl_seconds)
            .query_async::<()>(&mut *conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.pool.get().await.map_err(|e| CacheError::Backend(e.to_string()))?;

        cmd("DEL")
            .arg(&[self.prefixed_key(key)])
            .query_async::<()>(&mut *conn)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;

        Ok(())
    }
}
