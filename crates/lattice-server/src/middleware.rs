use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::SharedState;

/// Build a 401 JSON response matching the GraphQL error shape.
fn unauthorized(message: &str) -> Response {
    let body = serde_json::json!({
        "errors": [{ "message": message }],
    });

    (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
}

/// Determine the effective auth default for a given resource.
fn effective_default(state: &crate::AppState, resource: &str) -> Option<String> {
    if let Some(resources) = &state.auth_resources {
        let override_default = match resource {
            "mcp" => resources.mcp.as_ref().and_then(|r| r.default.clone()),
            "graphql" => resources.graphql.as_ref().and_then(|r| r.default.clone()),
            _ => None,
        };
        if override_default.is_some() {
            return override_default;
        }
    }
    state.auth_default.clone()
}

/// Determine the resource name from the request URI.
fn resource_from_uri(uri: &axum::http::Uri) -> &str {
    if uri.path().contains("/mcp") { "mcp" } else { "graphql" }
}

/// Auth middleware that validates JWTs and API keys.
///
/// When no auth is configured (no JWT validators and no API key validator),
/// requests pass through unchanged. Otherwise credentials are checked against
/// each JWT provider in order, then against API keys. If nothing matches, the
/// per-resource or global `auth_default` is consulted — "anonymous" passes
/// through, anything else returns 401.
pub async fn auth(State(shared): State<SharedState>, mut request: Request<axum::body::Body>, next: Next) -> Response {
    let state = shared.load_full();

    // If no auth configured at all, pass through.
    if state.jwt_validators.is_empty() && state.api_key_validator.is_none() {
        return next.run(request).await;
    }

    let resource = resource_from_uri(request.uri());

    // Try each JWT provider in order.
    for validator in &state.jwt_validators {
        if let Some(header_value) = request.headers().get("authorization").and_then(|v| v.to_str().ok()) {
            if let Ok(claims) = validator.validate(header_value).await {
                request.extensions_mut().insert(claims);
                return next.run(request).await;
            }
        }
    }

    // Try API key.
    if let Some(api_validator) = &state.api_key_validator {
        if let Some(key_value) = request
            .headers()
            .get(api_validator.header_name())
            .and_then(|v| v.to_str().ok())
        {
            if let Some(claims) = api_validator.validate(key_value) {
                request.extensions_mut().insert(claims);
                return next.run(request).await;
            }
        }
    }

    // No valid credentials. Check default behavior.
    let default = effective_default(&state, resource);
    if default.as_deref() == Some("anonymous") {
        return next.run(request).await;
    }

    unauthorized("missing or invalid credentials")
}
