pub mod prometheus;

use std::sync::Mutex;

use lattice_config::TelemetryConfig;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Holds the OTLP tracer provider so it can be shut down gracefully.
static TRACER_PROVIDER: Mutex<Option<SdkTracerProvider>> = Mutex::new(None);

/// Initialize the tracing subscriber with optional OTLP export.
///
/// Reads the `RUST_LOG` env var first, falls back to the config's `log_level`.
/// If `log_format` is `"json"`, uses a JSON formatter; otherwise uses the
/// pretty formatter. If `otlp_endpoint` is set, adds an OpenTelemetry layer.
///
/// Call once at startup.
pub fn init(config: Option<&TelemetryConfig>) {
    let log_level = config.map_or("info", |c| c.log_level.as_str());
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let use_json = config.is_some_and(|c| c.log_format == "json");

    // Build OTLP layer if endpoint configured.
    let otlp_layer =
        config
            .and_then(|c| c.otlp_endpoint.as_ref())
            .and_then(|endpoint| match build_otlp_layer(endpoint) {
                Ok(layer) => Some(layer),
                Err(e) => {
                    eprintln!("failed to initialize OTLP exporter: {e}");
                    None
                }
            });

    // Use `Option<Layer>` pattern for conditional formatting.
    let json_layer = use_json.then(|| tracing_subscriber::fmt::layer().json());
    let pretty_layer = (!use_json).then(tracing_subscriber::fmt::layer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .with(pretty_layer)
        .with(otlp_layer)
        .init();
}

/// Shut down the OpenTelemetry tracer provider, flushing pending spans.
pub fn shutdown() {
    if let Ok(mut guard) = TRACER_PROVIDER.lock() {
        if let Some(provider) = guard.take() {
            if let Err(e) = provider.shutdown() {
                eprintln!("failed to shut down tracer provider: {e}");
            }
        }
    }
}

/// Build an OpenTelemetry tracing layer that exports spans via OTLP/gRPC.
///
/// # Errors
///
/// Returns an error if the OTLP exporter cannot be built.
fn build_otlp_layer<S>(
    endpoint: &str,
) -> Result<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>, Box<dyn std::error::Error>>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()?,
        )
        .build();

    let tracer = provider.tracer("lattice");

    // Store the provider for graceful shutdown and register globally.
    if let Ok(mut guard) = TRACER_PROVIDER.lock() {
        *guard = Some(provider.clone());
    }
    opentelemetry::global::set_tracer_provider(provider);

    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}
