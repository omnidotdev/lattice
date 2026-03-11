use std::sync::Arc;

use axum::{
    extract::State,
    http::{Request, StatusCode},
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

/// Auth middleware that validates JWTs when auth is configured.
///
/// When no `jwt_validator` is present in state (auth not configured), requests
/// pass through unchanged. Otherwise, a valid `Authorization: Bearer <token>`
/// header is required (or the custom header configured via `header_name`).
/// On success the decoded [`lattice_auth::Claims`] are inserted into request
/// extensions.
pub async fn auth(State(state): State<Arc<AppState>>, mut request: Request<axum::body::Body>, next: Next) -> Response {
    let Some(ref validator) = state.jwt_validator else {
        // No auth configured — pass through.
        return next.run(request).await;
    };

    let header_name = state
        .config
        .auth
        .as_ref()
        .and_then(|a| a.jwt.as_ref())
        .map_or("authorization", |jwt| jwt.header_name.as_str());

    let Some(header_value) = request.headers().get(header_name).and_then(|v| v.to_str().ok()) else {
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
