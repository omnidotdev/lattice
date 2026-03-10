use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

#[derive(Parser)]
#[command(name = "lattice-gateway", about = "Lattice GraphQL federation gateway")]
struct Cli {
    /// Path to config file.
    #[arg(short, long, default_value = "lattice.yaml")]
    config: PathBuf,
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
    let state = Arc::new(state);
    let app = lattice_server::router(state);

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
