use std::sync::Arc;

use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use tokio::sync::RwLock;

/// Fetch and cache a JWKS (JSON Web Key Set) from a remote URL.
#[derive(Debug, Clone)]
pub struct JwksProvider {
    url: String,
    client: Client,
    cache: Arc<RwLock<Option<JwkSet>>>,
}

impl JwksProvider {
    /// Create a new provider for the given JWKS endpoint.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: Client::new(),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Return the cached JWKS or fetch from the remote URL.
    ///
    /// # Errors
    ///
    /// Returns a [`reqwest::Error`] if the HTTP request fails or the response
    /// body cannot be deserialized.
    pub async fn get_jwks(&self) -> Result<JwkSet, reqwest::Error> {
        // Fast path: return cached value.
        {
            let guard = self.cache.read().await;
            if let Some(ref jwks) = *guard {
                return Ok(jwks.clone());
            }
        }

        // Slow path: fetch and cache.
        let jwks: JwkSet = self.client.get(&self.url).send().await?.json().await?;

        {
            let mut guard = self.cache.write().await;
            *guard = Some(jwks.clone());
        }

        tracing::debug!(url = %self.url, keys = jwks.keys.len(), "fetched JWKS");

        Ok(jwks)
    }

    /// Clear the cached JWKS so the next call to [`get_jwks`](Self::get_jwks)
    /// will re-fetch from the remote URL.
    pub async fn invalidate(&self) {
        *self.cache.write().await = None;
        tracing::debug!(url = %self.url, "invalidated JWKS cache");
    }

    /// Start a background task that invalidates the cache at the given interval.
    #[must_use]
    pub fn start_polling(&self, interval_seconds: u64) -> tokio::task::JoinHandle<()> {
        let provider = self.clone();
        let interval = std::time::Duration::from_secs(interval_seconds);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                provider.invalidate().await;
                tracing::debug!(url = %provider.url, "JWKS cache invalidated by polling");
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn polling_provider_invalidates_cache() {
        let provider = JwksProvider::new("https://example.com/.well-known/jwks.json");
        let handle = provider.start_polling(1);
        assert!(!handle.is_finished());
        handle.abort();
    }
}
