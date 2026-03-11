use std::path::Path;
use std::sync::Arc;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServeError {
    #[error("failed to load config: {0}")]
    Config(#[from] lattice_config::ConfigError),
    #[error("failed to bind to {address}: {source}")]
    Bind {
        address: String,
        #[source]
        source: std::io::Error,
    },
    #[error("server error: {0}")]
    Server(#[source] std::io::Error),
}

/// Start the Lattice gateway server.
///
/// # Errors
///
/// Returns [`ServeError`] on config, bind, or runtime failures.
pub async fn run(config_path: &Path) -> Result<(), ServeError> {
    let config = lattice_config::load(config_path)?;

    lattice_telemetry::init(config.telemetry.as_ref());

    let supergraph_sdl = config
        .supergraph
        .path
        .as_ref()
        .and_then(|p| match std::fs::read_to_string(p) {
            Ok(sdl) => {
                tracing::info!(path = %p, "loaded supergraph SDL");
                Some(sdl)
            }
            Err(e) => {
                tracing::warn!(path = %p, "could not load supergraph SDL: {e}");
                None
            }
        });

    let plugin_manager = config
        .plugins
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(lattice_plugin::load_plugins)
        .transpose()
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to load plugins");
            None
        });

    let listen = config.server.listen.clone();
    let mut state = lattice_server::AppState::new(config, supergraph_sdl.as_deref());
    state.plugin_manager = plugin_manager;
    let state = Arc::new(state);
    let app = lattice_server::router(state);

    tracing::info!("starting lattice gateway on {listen}");
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| ServeError::Bind {
            address: listen,
            source: e,
        })?;

    axum::serve(listener, app).await.map_err(ServeError::Server)?;

    lattice_telemetry::shutdown();

    Ok(())
}
