mod graphql;
mod state;

use std::sync::Arc;

use axum::{Router, routing::post};

pub use state::AppState;

/// Build the axum router with all routes.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new().route("/graphql", post(graphql::handle)).with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState::new_stub())
    }

    #[tokio::test]
    async fn graphql_endpoint_accepts_post() {
        let app = router(test_state());
        let body = serde_json::json!({
            "query": "{ __typename }"
        });

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
        let body = serde_json::json!({
            "query": "{ __typename }"
        });

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
}
