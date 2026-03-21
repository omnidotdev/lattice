use axum::response::IntoResponse;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Shared handle for rendering the Prometheus scrape endpoint.
static PROM_HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();

/// Install the Prometheus metrics recorder.
///
/// Must be called once at startup before any metrics are recorded. Subsequent
/// calls are no-ops. Returns the renderer handle.
///
/// # Panics
///
/// Panics if the Prometheus recorder has already been installed by a
/// non-Lattice caller (conflicting global recorder).
pub fn install() -> &'static PrometheusHandle {
    PROM_HANDLE.get_or_init(|| {
        let builder = PrometheusBuilder::new();

        builder
            .install_recorder()
            .expect("prometheus recorder already installed")
    })
}

/// Axum handler that renders all collected metrics in Prometheus exposition
/// format.
///
/// # Panics
///
/// Panics if called before [`install`] has been invoked.
#[allow(clippy::unused_async)]
pub async fn metrics_handler() -> impl IntoResponse {
    let handle = PROM_HANDLE.get().expect("prometheus not installed");
    handle.render()
}

/// Record an HTTP request (counter + histogram).
pub fn record_http_request(method: &str, status: u16, operation_type: &str, duration_secs: f64) {
    metrics::counter!(
        "lattice_http_requests_total",
        "method" => method.to_owned(),
        "status" => status.to_string(),
        "operation_type" => operation_type.to_owned(),
    )
    .increment(1);

    metrics::histogram!(
        "lattice_http_request_duration_seconds",
        "operation_type" => operation_type.to_owned(),
    )
    .record(duration_secs);
}

/// Record a subgraph fetch (counter + histogram).
pub fn record_subgraph_request(subgraph: &str, status: u16, duration_secs: f64) {
    metrics::counter!(
        "lattice_subgraph_requests_total",
        "subgraph" => subgraph.to_owned(),
        "status" => status.to_string(),
    )
    .increment(1);

    metrics::histogram!(
        "lattice_subgraph_request_duration_seconds",
        "subgraph" => subgraph.to_owned(),
    )
    .record(duration_secs);
}

/// Record a cache hit.
pub fn record_cache_hit() {
    metrics::counter!("lattice_cache_hits_total").increment(1);
}

/// Record a cache miss.
pub fn record_cache_miss() {
    metrics::counter!("lattice_cache_misses_total").increment(1);
}

/// Record a rate-limit rejection.
pub fn record_rate_limit_rejected() {
    metrics::counter!("lattice_rate_limit_rejected_total").increment(1);
}

/// Increment the active subscriptions gauge.
pub fn inc_active_subscriptions() {
    metrics::gauge!("lattice_active_subscriptions").increment(1.0);
}

/// Decrement the active subscriptions gauge.
pub fn dec_active_subscriptions() {
    metrics::gauge!("lattice_active_subscriptions").decrement(1.0);
}
