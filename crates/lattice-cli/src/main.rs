mod check;
mod compose;
mod serve;

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

    /// Start the Lattice gateway server.
    Serve {
        /// Path to lattice.yaml config file.
        #[arg(short, long, default_value = "lattice.yaml")]
        config: PathBuf,
    },

    /// Validate a Lattice configuration file.
    Check {
        /// Path to lattice.yaml config file.
        #[arg(short, long, default_value = "lattice.yaml")]
        config: PathBuf,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    let result = run(cli.command);
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

fn run(command: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Compose { config, output } => compose::run(&config, &output)?,
        Commands::Serve { config } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run(&config))?;
        }
        Commands::Check { config } => check::run(&config)?,
    }
    Ok(())
}
