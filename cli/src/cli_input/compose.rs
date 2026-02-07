use clap::Parser;
use std::path::PathBuf;

/// Compose subgraph schemas into a federated supergraph
#[derive(Debug, Parser)]
pub struct ComposeCommand {
    /// Path to the supergraph config file (YAML)
    #[arg(short, long)]
    pub config: PathBuf,

    /// Output file path (defaults to stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}
