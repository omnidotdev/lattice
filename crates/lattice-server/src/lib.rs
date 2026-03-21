pub mod contracts;
mod cors;
mod csrf;
mod graphql;
mod limits;
mod mcp;
pub mod middleware;
mod playground;
mod rate_limit;
mod state;
mod subscriptions;

use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    response::Response,
    routing::{get, post},
};

pub use state::{AppState, SharedState};

/// Handle WebSocket upgrade for GraphQL subscriptions.
async fn ws_upgrade(ws: WebSocketUpgrade, State(shared): State<SharedState>) -> Response {
    let state = shared.load_full();
    ws.protocols(["graphql-transport-ws"])
        .on_upgrade(move |socket| subscriptions::handle_socket(socket, state))
}

/// Mount MCP routes if enabled in config.
fn mount_mcp(app: Router<SharedState>, state: &SharedState) -> Router<SharedState> {
    let Some(mcp_config) = &state.load().config.mcp else {
        return app;
    };

    if !mcp_config.enabled {
        return app;
    }

    let path = mcp_config.path.clone();
    tracing::info!(path = %path, "MCP server enabled");
    app.merge(Router::new().nest(&path, mcp::router()))
}

/// Mount the Prometheus `/metrics` endpoint if enabled in config.
fn mount_metrics(app: Router<SharedState>, state: &SharedState) -> Router<SharedState> {
    let enabled = state
        .load()
        .config
        .telemetry
        .as_ref()
        .and_then(|t| t.prometheus.as_ref())
        .is_some_and(|p| p.enabled);

    if !enabled {
        return app;
    }

    let prom = state
        .load()
        .config
        .telemetry
        .as_ref()
        .and_then(|t| t.prometheus.clone())
        .expect("checked above");

    lattice_telemetry::prometheus::install();
    tracing::info!(path = %prom.path, "prometheus metrics enabled");
    app.route(&prom.path, get(lattice_telemetry::prometheus::metrics_handler))
}

/// Build the axum router with all routes.
pub fn router(state: SharedState) -> Router {
    let mut app = Router::new().route("/graphql", post(graphql::handle).get(ws_upgrade));

    // Serve GraphiQL when playground is enabled (default: true).
    if state.load().config.server.playground.unwrap_or(true) {
        app = app.route("/", get(playground::graphiql));
    }

    // Apply CSRF protection (innermost — runs first on requests).
    app = app.layer(axum::middleware::from_fn_with_state(state.clone(), csrf::csrf));

    // Apply body size limits.
    app = app.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        limits::body_size_limit,
    ));

    // Apply auth + rate-limit middleware.
    app = app
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit,
        ));

    // Mount optional routes outside the auth/rate-limit stack.
    app = mount_mcp(app, &state);
    app = mount_metrics(app, &state);

    // Apply CORS layer (outermost — wraps all routes including MCP and metrics).
    if let Some(cors_config) = &state.load().config.cors {
        tracing::info!("CORS protection enabled");
        app = app.layer(cors::build_cors_layer(cors_config));
    }

    app.with_state(state)
}

#[cfg(test)]
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use arc_swap::ArcSwap;
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

    fn test_state() -> SharedState {
        std::sync::Arc::new(ArcSwap::from(std::sync::Arc::new(AppState::new(test_config(), None))))
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
    async fn api_key_auth_passes() {
        let config: lattice_config::Config = serde_yaml::from_str(
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
auth:
  default: deny
  api_keys:
    header_name: X-API-Key
    keys:
      - name: test-key
        value: secret-123
        claims:
          role: admin
"#,
        )
        .unwrap();

        let state = AppState::new(config, None);
        let state: SharedState = std::sync::Arc::new(ArcSwap::from(std::sync::Arc::new(state)));
        let app = router(state);

        let body = serde_json::json!({ "query": "{ __typename }" });

        // Without API key -> 401.
        let resp = app
            .clone()
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // With API key -> 200.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graphql")
                    .header("content-type", "application/json")
                    .header("X-API-Key", "secret-123")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_default_anonymous_passes_without_credentials() {
        let config: lattice_config::Config = serde_yaml::from_str(
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
auth:
  default: anonymous
  providers:
    - jwks_url: https://example.com/.well-known/jwks.json
"#,
        )
        .unwrap();

        let state = AppState::new(config, None);
        let state: SharedState = std::sync::Arc::new(ArcSwap::from(std::sync::Arc::new(state)));
        let app = router(state);

        let body = serde_json::json!({ "query": "{ __typename }" });
        let resp = app
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
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rate_limit_returns_429() {
        let limiter = std::sync::Arc::new(InMemoryRateLimiter::new());

        let mut state = AppState::new(test_config(), None);
        state.rate_limiter = Some(limiter);
        state.rate_limit_defaults = Some((1, 60));
        let state: SharedState = std::sync::Arc::new(ArcSwap::from(std::sync::Arc::new(state)));
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

    #[tokio::test]
    async fn contracts_config_builds_registry() {
        let config: lattice_config::Config = serde_yaml::from_str(
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
contracts:
  default: full
  variants:
    public:
      exclude:
        - internal
"#,
        )
        .unwrap();

        // Need a valid supergraph SDL with tags to build the registry.
        let sdl = r#"
            schema
                @link(url: "https://specs.apollo.dev/link/v1.0")
                @link(url: "https://specs.apollo.dev/join/v0.3", for: EXECUTION)
            {
                query: Query
            }

            directive @tag(name: String!) repeatable on FIELD_DEFINITION | OBJECT
            directive @join__enumValue(graph: join__Graph!) repeatable on ENUM_VALUE
            directive @join__field(graph: join__Graph, requires: join__FieldSet, provides: join__FieldSet, type: String, external: Boolean, override: String, usedOverridden: Boolean) repeatable on FIELD_DEFINITION | INPUT_FIELD_DEFINITION
            directive @join__graph(name: String!, url: String!) on ENUM_VALUE
            directive @join__implements(graph: join__Graph!, interface: String!) repeatable on OBJECT | INTERFACE
            directive @join__type(graph: join__Graph!, key: join__FieldSet, extension: Boolean! = false, resolvable: Boolean! = true, isInterfaceObject: Boolean! = false) repeatable on OBJECT | INTERFACE | UNION | ENUM | INPUT_OBJECT | SCALAR
            directive @join__unionMember(graph: join__Graph!, member: String!) repeatable on UNION
            directive @link(url: String, as: String, for: link__Purpose, import: [link__Import]) repeatable on SCHEMA

            scalar join__FieldSet
            scalar link__Import
            enum link__Purpose { SECURITY EXECUTION }
            enum join__Graph {
                USERS @join__graph(name: "users", url: "http://users:3000/graphql")
            }

            type Query @join__type(graph: USERS) {
                me: User @join__field(graph: USERS)
            }

            type User @join__type(graph: USERS, key: "id") {
                id: ID!
                name: String
                email: String @tag(name: "internal")
            }
        "#;

        let state = AppState::new(config, Some(sdl));
        assert!(state.contract_registry.is_some());
        let registry = state.contract_registry.as_ref().unwrap();
        assert!(registry.get("full").is_some());
        assert!(registry.get("public").is_some());
    }
}
