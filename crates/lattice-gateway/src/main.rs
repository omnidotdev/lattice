use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use clap::Parser;

use lattice_server::SharedState;

#[derive(Parser)]
#[command(name = "lattice-gateway", about = "Lattice GraphQL federation gateway")]
struct Cli {
    /// Path to config file.
    #[arg(short, long, default_value = "lattice.yaml")]
    config: PathBuf,

    /// Watch config file for changes and reload without restart.
    #[arg(long)]
    hot_reload: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config = match lattice_config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to load config: {e}");
            std::process::exit(1);
        }
    };

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
    let _watcher = if cli.hot_reload {
        let (notify_tx, mut notify_rx) = tokio::sync::watch::channel(());
        let reload_path = cli.config.clone();
        let shared_clone = shared.clone();

        let watcher = match lattice_config::ConfigWatcher::new(cli.config.clone(), move || {
            let _ = notify_tx.send(());
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "failed to start config watcher");
                std::process::exit(1);
            }
        };

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
    let listener = match tokio::net::TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind to {listen}: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("server error: {e}");
        std::process::exit(1);
    }

    lattice_telemetry::shutdown();
}

/// Rebuild `AppState` from a config file on disk.
fn rebuild_state(config_path: &std::path::Path) -> Result<lattice_server::AppState, Box<dyn std::error::Error>> {
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
