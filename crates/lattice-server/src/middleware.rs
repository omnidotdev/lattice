use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::AppState;

/// Build a 401 JSON response matching the GraphQL error shape.
fn unauthorized(message: &str) -> Response {
    let body = serde_json::json!({
        "errors": [{ "message": message }],
    });

    (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
}

/// Extract the `Authorization` header value from request headers.
fn extract_auth_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
}

/// Auth middleware that validates JWTs when auth is configured.
///
/// When no `jwt_validator` is present in state (auth not configured), requests
/// pass through unchanged. Otherwise, a valid `Authorization: Bearer <token>`
/// header is required. On success the decoded [`lattice_auth::Claims`] are
/// inserted into request extensions.
pub async fn auth(State(state): State<Arc<AppState>>, mut request: Request<axum::body::Body>, next: Next) -> Response {
    let Some(ref validator) = state.jwt_validator else {
        // No auth configured — pass through.
        return next.run(request).await;
    };

    let Some(header_value) = extract_auth_header(request.headers()) else {
        return unauthorized("missing authorization header");
    };

    match validator.validate(header_value).await {
        Ok(claims) => {
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Err(e) => unauthorized(&e.to_string()),
    }
}
