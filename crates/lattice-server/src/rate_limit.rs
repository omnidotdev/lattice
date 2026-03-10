use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::AppState;

/// Rate limiting middleware.
///
/// Extract client IP from the `x-forwarded-for` header and check the rate
/// limiter. Return 429 Too Many Requests when the limit is exceeded.
/// Fail open if the rate limiter encounters an error.
pub async fn rate_limit(State(state): State<Arc<AppState>>, request: Request<Body>, next: Next) -> Response {
    let Some(limiter) = &state.rate_limiter else {
        return next.run(request).await;
    };

    let Some((max_requests, window_seconds)) = state.rate_limit_defaults else {
        return next.run(request).await;
    };

    // Use x-forwarded-for or fall back to "unknown".
    let key = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map_or_else(
            || "unknown".to_string(),
            |s| s.split(',').next().unwrap_or(s).trim().to_string(),
        );

    match limiter.check(&key, max_requests, window_seconds).await {
        Ok(result) if result.allowed => next.run(request).await,
        Ok(result) => {
            let body = serde_json::json!({
                "errors": [{
                    "message": format!(
                        "rate limit exceeded: {} requests per window, retry after {}s",
                        result.limit,
                        result.reset_after,
                    )
                }]
            });

            (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response()
        }
        Err(e) => {
            // Fail open: rate limiter errors should not block requests.
            tracing::warn!(error = %e, "rate limiter error, allowing request");
            next.run(request).await
        }
    }
}
