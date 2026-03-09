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
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    let config = match lattice_config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to load config: {e}");
            std::process::exit(1);
        }
    };

    let listen = config.server.listen.clone();
    let state = Arc::new(lattice_server::AppState::from_config(config));
    let app = lattice_server::router(state);

    tracing::info!("starting lattice gateway on {listen}");
    let listener = tokio::net::TcpListener::bind(&listen).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
