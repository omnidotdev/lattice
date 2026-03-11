use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use thiserror::Error;

use lattice_server::SharedState;

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

/// Rebuild `AppState` from a config file on disk.
///
/// Used during hot-reload to create a fresh state from the updated config.
fn rebuild_state(config_path: &Path) -> Result<lattice_server::AppState, ServeError> {
    let config = lattice_config::load(config_path)?;

    let supergraph_sdl = config.supergraph.path.as_ref().and_then(|p| {
        std::fs::read_to_string(p)
            .map_err(|e| {
                tracing::warn!(path = %p, "could not load supergraph SDL during reload: {e}");
                e
            })
            .ok()
    });

    let plugin_manager = config
        .plugins
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(lattice_plugin::load_plugins)
        .transpose()
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to load plugins during hot-reload");
            None
        });

    let mut state = lattice_server::AppState::new(config, supergraph_sdl.as_deref());
    state.plugin_manager = plugin_manager;

    Ok(state)
}

/// Start the Lattice gateway server.
///
/// # Errors
///
/// Returns [`ServeError`] on config, bind, or runtime failures.
pub async fn run(config_path: &Path, hot_reload: bool) -> Result<(), ServeError> {
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

    let shared: SharedState = Arc::new(ArcSwap::from(Arc::new(state)));

    // Start config file watcher for hot-reload.
    let _watcher = if hot_reload {
        let (notify_tx, mut notify_rx) = tokio::sync::watch::channel(());
        let reload_path = config_path.to_path_buf();
        let shared_clone = shared.clone();

        let watcher = match lattice_config::ConfigWatcher::new(config_path.to_path_buf(), move || {
            let _ = notify_tx.send(());
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "failed to start config watcher, cannot enable hot-reload");
                return Err(ServeError::Server(std::io::Error::other(format!(
                    "config watcher failed: {e}"
                ))));
            }
        };

        // Spawn the rebuild task.
        tokio::spawn(async move {
            while notify_rx.changed().await.is_ok() {
                match rebuild_state(&reload_path) {
                    Ok(new_state) => {
                        shared_clone.store(Arc::new(new_state));
                        tracing::info!("config hot-reloaded successfully");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "config hot-reload failed, keeping current state");
                    }
                }
            }
        });

        Some(watcher)
    } else {
        None
    };

    let app = lattice_server::router(shared);

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
