use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::watch;

use crate::dispatch::SubgraphResponse;

/// Hash key for deduplication: derived from (url, query, variables, `operation_name`).
type DedupKey = u64;

/// Shared result type for in-flight request watchers.
type InflightResult = Result<SubgraphResponse, String>;

/// Coalesce identical in-flight subgraph requests.
///
/// When multiple concurrent fetches target the same subgraph with identical
/// query, variables, and operation name, only one HTTP request is dispatched.
/// Subsequent callers wait for the result of the first request.
#[derive(Debug, Clone)]
pub struct DedupLayer {
    inflight: Arc<DashMap<DedupKey, watch::Receiver<Option<InflightResult>>>>,
}

impl DedupLayer {
    /// Create a new deduplication layer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inflight: Arc::new(DashMap::new()),
        }
    }

    /// Compute a dedup key from request components.
    #[must_use]
    pub fn hash_request(
        url: &str,
        query: &str,
        variables: Option<&serde_json::Value>,
        operation_name: Option<&str>,
    ) -> DedupKey {
        use xxhash_rust::xxh3::xxh3_64;

        let mut buf = Vec::with_capacity(url.len() + query.len() + 64);
        buf.extend_from_slice(url.as_bytes());
        buf.push(0xFF);
        buf.extend_from_slice(query.as_bytes());
        buf.push(0xFF);
        if let Some(vars) = variables {
            buf.extend_from_slice(vars.to_string().as_bytes());
        }
        buf.push(0xFF);
        if let Some(op) = operation_name {
            buf.extend_from_slice(op.as_bytes());
        }

        xxh3_64(&buf)
    }

    /// Try to acquire a "flight ticket" for a request.
    ///
    /// Returns `DedupAction::Execute` if this caller should execute the
    /// request (first caller wins). Returns `DedupAction::Wait` if another
    /// caller is already executing the same request.
    #[must_use]
    pub fn try_acquire(&self, key: DedupKey) -> DedupAction {
        if let Some(entry) = self.inflight.get(&key) {
            return DedupAction::Wait(entry.value().clone());
        }

        let (tx, rx) = watch::channel(None);
        self.inflight.insert(key, rx);
        DedupAction::Execute(DedupTicket {
            key,
            sender: tx,
            inflight: Arc::clone(&self.inflight),
        })
    }
}

impl Default for DedupLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of `try_acquire`.
pub enum DedupAction {
    /// This caller should execute the request and complete the ticket.
    Execute(DedupTicket),
    /// Another caller is already executing. Wait on this receiver.
    Wait(watch::Receiver<Option<InflightResult>>),
}

impl std::fmt::Debug for DedupAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Execute(_) => f.debug_tuple("Execute").finish(),
            Self::Wait(_) => f.debug_tuple("Wait").finish(),
        }
    }
}

/// Ticket held by the caller executing the request.
///
/// On `complete`, broadcasts the result to all waiters and removes the
/// inflight entry. If dropped without completing, the entry is cleaned up
/// and waiters receive an error.
pub struct DedupTicket {
    key: DedupKey,
    sender: watch::Sender<Option<InflightResult>>,
    inflight: Arc<DashMap<DedupKey, watch::Receiver<Option<InflightResult>>>>,
}

impl std::fmt::Debug for DedupTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DedupTicket")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl DedupTicket {
    /// Broadcast the result to all waiters and remove the inflight entry.
    pub fn complete(self, result: InflightResult) {
        let _ = self.sender.send(Some(result));
        self.inflight.remove(&self.key);
    }
}

impl Drop for DedupTicket {
    fn drop(&mut self) {
        // Clean up if dropped without completing (e.g. panic).
        self.inflight.remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_produces_consistent_keys() {
        let key1 = DedupLayer::hash_request(
            "http://users/graphql",
            "{ me { id } }",
            Some(&json!({"id": "1"})),
            Some("GetMe"),
        );
        let key2 = DedupLayer::hash_request(
            "http://users/graphql",
            "{ me { id } }",
            Some(&json!({"id": "1"})),
            Some("GetMe"),
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn hash_differs_for_different_queries() {
        let key1 = DedupLayer::hash_request("http://users/graphql", "{ me { id } }", None, None);
        let key2 = DedupLayer::hash_request("http://users/graphql", "{ me { name } }", None, None);
        assert_ne!(key1, key2);
    }

    #[test]
    fn hash_differs_for_different_urls() {
        let key1 = DedupLayer::hash_request("http://users/graphql", "{ me }", None, None);
        let key2 = DedupLayer::hash_request("http://products/graphql", "{ me }", None, None);
        assert_ne!(key1, key2);
    }

    #[test]
    fn first_caller_gets_execute() {
        let layer = DedupLayer::new();
        let key = DedupLayer::hash_request("http://a/graphql", "{ me }", None, None);

        match layer.try_acquire(key) {
            DedupAction::Execute(_ticket) => {} // expected
            DedupAction::Wait(_) => panic!("expected Execute for first caller"),
        }
    }

    #[test]
    fn second_caller_gets_wait() {
        let layer = DedupLayer::new();
        let key = DedupLayer::hash_request("http://a/graphql", "{ me }", None, None);

        let _ticket = match layer.try_acquire(key) {
            DedupAction::Execute(ticket) => ticket,
            DedupAction::Wait(_) => panic!("expected Execute"),
        };

        match layer.try_acquire(key) {
            DedupAction::Wait(_) => {} // expected
            DedupAction::Execute(_) => panic!("expected Wait for second caller"),
        }
    }

    #[tokio::test]
    async fn waiter_receives_result_on_complete() {
        let layer = DedupLayer::new();
        let key = DedupLayer::hash_request("http://a/graphql", "{ me }", None, None);

        let ticket = match layer.try_acquire(key) {
            DedupAction::Execute(t) => t,
            DedupAction::Wait(_) => panic!("expected Execute"),
        };

        let mut rx = match layer.try_acquire(key) {
            DedupAction::Wait(rx) => rx,
            DedupAction::Execute(_) => panic!("expected Wait"),
        };

        let response = crate::dispatch::SubgraphResponse {
            data: Some(json!({"me": {"id": "1"}})),
            errors: None,
        };

        ticket.complete(Ok(response));

        rx.changed().await.unwrap();
        let binding = rx.borrow();
        let result = binding.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(result.data, Some(json!({"me": {"id": "1"}})));
    }

    #[test]
    fn entry_cleaned_up_after_complete() {
        let layer = DedupLayer::new();
        let key = DedupLayer::hash_request("http://a/graphql", "{ me }", None, None);

        let ticket = match layer.try_acquire(key) {
            DedupAction::Execute(t) => t,
            DedupAction::Wait(_) => panic!("expected Execute"),
        };

        let response = crate::dispatch::SubgraphResponse {
            data: None,
            errors: None,
        };
        ticket.complete(Ok(response));

        // After complete, next caller should get Execute again.
        match layer.try_acquire(key) {
            DedupAction::Execute(_) => {} // expected
            DedupAction::Wait(_) => panic!("expected Execute after completion"),
        }
    }
}
