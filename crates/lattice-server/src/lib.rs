mod graphql;
pub mod middleware;
mod playground;
mod rate_limit;
mod state;
mod subscriptions;

use std::sync::Arc;

use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    response::Response,
    routing::{get, post},
};

pub use state::AppState;

/// Handle WebSocket upgrade for GraphQL subscriptions.
async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.protocols(["graphql-transport-ws"])
        .on_upgrade(move |socket| subscriptions::handle_socket(socket, state))
}

/// Build the axum router with all routes.
pub fn router(state: Arc<AppState>) -> Router {
    let mut app = Router::new().route("/graphql", post(graphql::handle).get(ws_upgrade));

    // Serve GraphiQL when playground is enabled (default: true).
    let playground_enabled = state.config.server.playground.unwrap_or(true);

    if playground_enabled {
        app = app.route("/", get(playground::graphiql));
    }

    app.layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit,
        ))
        .with_state(state)
}

#[cfg(test)]
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use lattice_rate_limit::{RateLimitError, RateLimitResult, RateLimiter};
    use tower::ServiceExt;

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

        fn now_secs() -> u64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_secs()
        }
    }

    impl RateLimiter for InMemoryRateLimiter {
        fn check(
            &self,
            key: &str,
            max_requests: u64,
            window_seconds: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RateLimitResult, RateLimitError>> + Send + '_>>
        {
            let key = key.to_owned();

            Box::pin(async move {
                let mut store = self.store.lock().map_err(|e| RateLimitError::Backend(e.to_string()))?;

                let now = Self::now_secs();
                let window_start = now - (now % window_seconds);

                let state = store
                    .entry(key)
                    .or_insert_with(|| WindowState { count: 0, window_start });

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
            })
        }
    }

    fn test_config() -> lattice_config::Config {
        serde_yaml::from_str(
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
"#,
        )
        .unwrap()
    }

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState::new(test_config(), None))
    }

    #[tokio::test]
    async fn graphql_endpoint_accepts_post() {
        let app = router(test_state());
        let body = serde_json::json!({ "query": "{ __typename }" });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn graphql_endpoint_returns_json() {
        let app = router(test_state());
        let body = serde_json::json!({ "query": "{ __typename }" });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(content_type.contains("application/json"));
    }

    #[tokio::test]
    async fn returns_error_without_supergraph() {
        let app = router(test_state());
        let body = serde_json::json!({ "query": "{ me { id } }" });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["errors"].is_array());
    }

    #[tokio::test]
    async fn serves_graphiql_playground() {
        let state = test_state();
        let app = router(state);

        let response = app
            .oneshot(Request::builder().method("GET").uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(content_type.contains("text/html"));

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("GraphiQL"));
    }

    #[tokio::test]
    async fn rate_limit_returns_429() {
        let limiter = Arc::new(InMemoryRateLimiter::new());

        let mut state = AppState::new(test_config(), None);
        state.rate_limiter = Some(limiter);
        state.rate_limit_defaults = Some((1, 60));
        let state = Arc::new(state);
        let app = router(state);

        let body = serde_json::json!({ "query": "{ __typename }" });
        let make_request = || {
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        };

        // First request succeeds.
        let resp = app.clone().oneshot(make_request()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second request is rate limited.
        let resp = app.oneshot(make_request()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
