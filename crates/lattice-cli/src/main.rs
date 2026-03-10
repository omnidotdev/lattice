mod compose;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lattice", about = "Lattice GraphQL federation gateway CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compose subgraph schemas into a supergraph SDL.
    Compose {
        /// Path to lattice.yaml config file.
        #[arg(short, long, default_value = "lattice.yaml")]
        config: PathBuf,

        /// Output path for the composed supergraph SDL.
        #[arg(short, long, default_value = "supergraph.graphql")]
        output: PathBuf,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Compose { config, output } => {
            if let Err(e) = compose::run(&config, &output) {
                tracing::error!("{e}");
                std::process::exit(1);
            }
        }
    }
}
