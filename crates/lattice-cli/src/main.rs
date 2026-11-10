mod check;
mod compose;
mod grpc;
mod odata;
mod openapi;
mod postgres;
mod serve;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lattice", version, about = "Lattice GraphQL federation gateway CLI")]
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

        /// Watch config file for changes and reload without restart.
        #[arg(long)]
        hot_reload: bool,
    },

    /// Validate a Lattice configuration file.
    Check {
        /// Path to lattice.yaml config file.
        #[arg(short, long, default_value = "lattice.yaml")]
        config: PathBuf,
    },

    /// Generate a REST subgraph (SDL + connector config) from an OpenAPI spec.
    Openapi {
        /// Path to the OpenAPI 3.x spec (JSON or YAML).
        spec: PathBuf,

        /// Output path for the generated GraphQL SDL.
        #[arg(short, long, default_value = "openapi.graphql")]
        output: PathBuf,

        /// Subgraph name used in the printed config snippet.
        #[arg(short, long, default_value = "rest")]
        name: String,

        /// Override the base URL (otherwise taken from the spec `servers`).
        #[arg(long)]
        base_url: Option<String>,
    },

    /// Generate a gRPC subgraph (SDL + connector config) from a protobuf
    /// descriptor set (`protoc --descriptor_set_out`).
    Grpc {
        /// Path to the compiled protobuf `FileDescriptorSet`.
        descriptor_set: PathBuf,

        /// Output path for the generated GraphQL SDL.
        #[arg(short, long, default_value = "grpc.graphql")]
        output: PathBuf,

        /// Subgraph name used in the printed config snippet.
        #[arg(short, long, default_value = "grpc")]
        name: String,

        /// gRPC server endpoint to call at request time.
        #[arg(short, long, default_value = "http://localhost:50051")]
        endpoint: String,
    },

    /// Generate a REST subgraph (SDL + connector config) from an OData 4.0
    /// `$metadata` (EDMX) document.
    Odata {
        /// Path to the OData `$metadata` document (EDMX XML).
        metadata: PathBuf,

        /// Output path for the generated GraphQL SDL.
        #[arg(short, long, default_value = "odata.graphql")]
        output: PathBuf,

        /// Subgraph name used in the printed config snippet.
        #[arg(short, long, default_value = "odata")]
        name: String,

        /// OData service root URL that entity-set paths are appended to.
        #[arg(short, long, default_value = "http://localhost/odata")]
        base_url: String,
    },

    /// Generate a subgraph (SDL + connector config) by introspecting a Postgres
    /// database.
    Postgres {
        /// Postgres connection URL (`postgres://user:pass@host:5432/dbname`).
        url: String,

        /// Output path for the generated GraphQL SDL.
        #[arg(short, long, default_value = "postgres.graphql")]
        output: PathBuf,

        /// Subgraph name used in the printed config snippet.
        #[arg(short, long, default_value = "postgres")]
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // `serve` installs its own tracing subscriber via lattice-telemetry (env
    // filter plus optional JSON and OTLP layers). Installing a basic subscriber
    // here as well would panic on the second global-default set, so only the
    // non-serve commands get the basic logger.
    if !matches!(cli.command, Commands::Serve { .. }) {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    let result = run(cli.command);
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

fn run(command: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Compose { config, output } => compose::run(&config, &output)?,
        Commands::Serve { config, hot_reload } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run(&config, hot_reload))?;
        }
        Commands::Check { config } => check::run(&config)?,
        Commands::Openapi {
            spec,
            output,
            name,
            base_url,
        } => openapi::run(&spec, &output, &name, base_url.as_deref())?,
        Commands::Grpc {
            descriptor_set,
            output,
            name,
            endpoint,
        } => grpc::run(&descriptor_set, &output, &name, &endpoint)?,
        Commands::Odata {
            metadata,
            output,
            name,
            base_url,
        } => odata::run(&metadata, &output, &name, &base_url)?,
        Commands::Postgres { url, output, name } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(postgres::run(&url, &output, &name))?;
        }
    }
    Ok(())
}
