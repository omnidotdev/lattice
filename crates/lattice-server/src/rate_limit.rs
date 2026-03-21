use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::SharedState;

/// Rate limiting middleware.
///
/// Extract client IP from the `x-forwarded-for` header and check the rate
/// limiter. Return 429 Too Many Requests when the limit is exceeded.
/// Fail open if the rate limiter encounters an error.
pub async fn rate_limit(State(shared): State<SharedState>, request: Request<Body>, next: Next) -> Response {
    let state = shared.load_full();

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
            lattice_telemetry::prometheus::record_rate_limit_rejected();
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
