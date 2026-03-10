mod valkey;

pub use valkey::ValkeyCache;

/// Errors that can occur during cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// An error from the underlying cache backend.
    #[error("cache backend error: {0}")]
    Backend(String),
}

/// Trait for async entity caching.
pub trait EntityCache: Send + Sync {
    /// Retrieve a cached value by key.
    fn get(&self, key: &str) -> impl Future<Output = Result<Option<String>, CacheError>> + Send;

    /// Store a value with the given key and TTL.
    fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> impl Future<Output = Result<(), CacheError>> + Send;

    /// Remove a cached value by key.
    fn delete(&self, key: &str) -> impl Future<Output = Result<(), CacheError>> + Send;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// In-memory cache for testing purposes.
    struct InMemoryCache {
        store: Mutex<HashMap<String, String>>,
    }

    impl InMemoryCache {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    impl EntityCache for InMemoryCache {
        async fn get(&self, key: &str) -> Result<Option<String>, CacheError> {
            let store = self.store.lock().map_err(|e| CacheError::Backend(e.to_string()))?;
            Ok(store.get(key).cloned())
        }

        async fn set(&self, key: &str, value: &str, _ttl_seconds: u64) -> Result<(), CacheError> {
            let mut store = self.store.lock().map_err(|e| CacheError::Backend(e.to_string()))?;
            store.insert(key.to_owned(), value.to_owned());
            Ok(())
        }

        async fn delete(&self, key: &str) -> Result<(), CacheError> {
            let mut store = self.store.lock().map_err(|e| CacheError::Backend(e.to_string()))?;
            store.remove(key);
            Ok(())
        }
    }

    #[tokio::test]
    async fn get_returns_none_for_missing() {
        let cache = InMemoryCache::new();
        let result = cache.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let cache = InMemoryCache::new();
        cache.set("key", "value", 60).await.unwrap();
        let result = cache.get("key").await.unwrap();
        assert_eq!(result.as_deref(), Some("value"));
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let cache = InMemoryCache::new();
        cache.set("key", "value", 60).await.unwrap();
        cache.delete("key").await.unwrap();
        let result = cache.get("key").await.unwrap();
        assert!(result.is_none());
    }
}
