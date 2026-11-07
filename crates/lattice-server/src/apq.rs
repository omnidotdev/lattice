//! Automatic Persisted Queries (APQ).
//!
//! Clients may send an operation's SHA-256 hash in place of the full query to
//! save bandwidth. The first time the gateway sees a hash it does not know it
//! asks the client to resend with the query body, verifies that the body hashes
//! to the claimed value, and stores the mapping. Subsequent requests send only
//! the hash. The store is bounded so untrusted clients cannot grow it without
//! limit, and the hash is always recomputed from the body before storing so a
//! client cannot poison the cache with a mismatched hash.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

/// Outcome of resolving an APQ request.
#[derive(Debug, PartialEq, Eq)]
pub enum ApqOutcome {
    /// Query text to execute, supplied inline or recovered from the store.
    Resolved(String),
    /// The hash is unknown and no query was supplied; the client must retry with
    /// the full query body.
    NotFound,
    /// A query was supplied whose SHA-256 did not match the supplied hash.
    HashMismatch,
}

/// Bounded map from a query's lowercase hex SHA-256 to the query text.
pub struct ApqStore {
    inner: Mutex<Inner>,
    capacity: usize,
}

struct Inner {
    map: HashMap<String, String>,
    order: VecDeque<String>,
}

impl ApqStore {
    /// Create a store holding at most `capacity` entries (clamped to at least 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            capacity: capacity.max(1),
        }
    }

    /// Look up a stored query by hash.
    pub fn get(&self, hash: &str) -> Option<String> {
        self.inner.lock().expect("apq store poisoned").map.get(hash).cloned()
    }

    /// Store a hash to query mapping, evicting the oldest entry when full. A hash
    /// already present is left untouched (the mapping is immutable: one hash
    /// always maps to one query body).
    #[allow(clippy::significant_drop_tightening)]
    pub fn insert(&self, hash: String, query: String) {
        let mut inner = self.inner.lock().expect("apq store poisoned");
        if inner.map.contains_key(&hash) {
            return;
        }
        inner.map.insert(hash.clone(), query);
        inner.order.push_back(hash);
        while inner.map.len() > self.capacity {
            if let Some(oldest) = inner.order.pop_front() {
                inner.map.remove(&oldest);
            }
        }
    }

    /// Number of stored entries.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("apq store poisoned").map.len()
    }
}

/// Compute the lowercase hex SHA-256 of `input`.
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Resolve an APQ request. Call only when a `persistedQuery` extension is
/// present: `hash` is its `sha256Hash` and `query` the operation body when the
/// client also sent one.
pub fn resolve_apq(query: Option<&str>, hash: &str, store: &ApqStore) -> ApqOutcome {
    query.map_or_else(
        // Hash only: recover the registered query or ask the client to retry.
        || store.get(hash).map_or(ApqOutcome::NotFound, ApqOutcome::Resolved),
        // Query supplied: verify the body hashes to the claimed value before
        // trusting (and storing) it, so a client cannot poison the cache.
        |body| {
            if sha256_hex(body) == hash {
                store.insert(hash.to_string(), body.to_string());
                ApqOutcome::Resolved(body.to_string())
            } else {
                ApqOutcome::HashMismatch
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vector() {
        // Canonical SHA-256 test vector for the input "abc".
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unknown_hash_without_query_is_not_found() {
        let store = ApqStore::new(10);
        assert_eq!(resolve_apq(None, "deadbeef", &store), ApqOutcome::NotFound);
    }

    #[test]
    fn matching_hash_with_query_registers_and_resolves() {
        let store = ApqStore::new(10);
        let query = "{ me { id } }";
        let hash = sha256_hex(query);

        assert_eq!(
            resolve_apq(Some(query), &hash, &store),
            ApqOutcome::Resolved(query.to_string())
        );
        assert_eq!(store.get(&hash), Some(query.to_string()));
    }

    #[test]
    fn registered_query_resolves_by_hash_on_subsequent_request() {
        let store = ApqStore::new(10);
        let query = "{ me { id } }";
        let hash = sha256_hex(query);

        let _ = resolve_apq(Some(query), &hash, &store);

        assert_eq!(
            resolve_apq(None, &hash, &store),
            ApqOutcome::Resolved(query.to_string())
        );
    }

    #[test]
    fn mismatched_hash_with_query_is_rejected_and_not_stored() {
        let store = ApqStore::new(10);
        let query = "{ me { id } }";
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";

        assert_eq!(resolve_apq(Some(query), wrong, &store), ApqOutcome::HashMismatch);
        assert_eq!(store.get(wrong), None);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn store_evicts_oldest_when_over_capacity() {
        let store = ApqStore::new(2);
        store.insert("a".into(), "qa".into());
        store.insert("b".into(), "qb".into());
        store.insert("c".into(), "qc".into());

        assert_eq!(store.len(), 2);
        assert_eq!(store.get("a"), None);
        assert_eq!(store.get("b"), Some("qb".into()));
        assert_eq!(store.get("c"), Some("qc".into()));
    }
}
