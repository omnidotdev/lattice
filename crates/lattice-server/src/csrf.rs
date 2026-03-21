use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::SharedState;

/// CSRF protection middleware.
///
/// Reject non-safe HTTP methods that are missing any of the configured
/// required headers. Safe methods (GET, HEAD, OPTIONS) are always allowed
/// through so that preflight requests and playground fetches work.
pub async fn csrf(State(shared): State<SharedState>, request: Request<Body>, next: Next) -> Response {
    let state = shared.load_full();

    let Some(csrf_config) = &state.config.csrf else {
        return next.run(request).await;
    };

    // Allow safe methods without CSRF headers.
    let method = request.method().clone();
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return next.run(request).await;
    }

    for required in &csrf_config.required_headers {
        if !request.headers().contains_key(required.as_str()) {
            let body = serde_json::json!({
                "errors": [{
                    "message": format!("missing required CSRF header: {required}")
                }]
            });

            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }
    }

    next.run(request).await
}
