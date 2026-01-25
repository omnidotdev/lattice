mod config;

pub(crate) use config::ComposeConfig;

use crate::{cli_input::ComposeCommand, errors::CliError};
use graphql_composition::{compose, render_federated_sdl, Subgraphs};
use std::{fs, io::Write};
use tokio::runtime::Runtime;

/// Execute the compose command.
///
/// Loads the compose config, fetches schemas from all subgraphs,
/// composes them into a federated supergraph, and outputs the SDL.
///
/// # Errors
///
/// Returns an error if config loading, schema fetching, or composition fails.
pub fn compose_command(cmd: ComposeCommand) -> Result<(), CliError> {
    let config = ComposeConfig::load(&cmd.config)?;
    let mut subgraphs = Subgraphs::default();

    let rt = Runtime::new().unwrap();

    for (name, subgraph) in &config.subgraphs {
        let schema = rt.block_on(subgraph.fetch_schema(&config.headers))?;
        subgraphs
            .ingest_str(&schema, name, subgraph.routing_url())
            .map_err(|e| CliError::CompositionFailed(e.to_string()))?;
    }

    let result = compose(&subgraphs);

    // Print diagnostics to stderr.
    for message in result.diagnostics().iter_messages() {
        eprintln!("{message}");
    }

    // Convert to result, handling errors.
    let federated_graph = match result.into_result() {
        Ok(graph) => graph
            .into_latest()
            .expect("graph should be convertible to latest version"),
        Err(diagnostics) => {
            let errors: Vec<_> = diagnostics.iter_messages().collect();
            return Err(CliError::CompositionFailed(errors.join("\n")));
        }
    };

    let sdl = render_federated_sdl(&federated_graph)
        .map_err(|e| CliError::CompositionFailed(format!("failed to render SDL: {e}")))?;

    match cmd.output {
        Some(ref path) => {
            fs::write(path, &sdl).map_err(|e| CliError::WriteComposeOutput(path.clone(), e))?;
            eprintln!(
                "Composed {} subgraphs into {}",
                config.subgraphs.len(),
                path.display()
            );
        }
        None => {
            std::io::stdout().write_all(sdl.as_bytes()).unwrap();
        }
    }

    Ok(())
}
