use std::{collections::BTreeMap, fs, io, path::Path};

use lattice_config::PostgresConnectorConfig;
use lattice_connectors::postgres;
use serde::Serialize;
use thiserror::Error;

/// Errors raised while generating a subgraph from a Postgres database.
#[derive(Debug, Error)]
pub enum PostgresCliError {
    #[error("failed to connect to postgres: {0}")]
    Connect(String),
    #[error("failed to introspect schema: {0}")]
    Introspect(String),
    #[error("failed to write schema: {0}")]
    Write(#[source] io::Error),
}

/// A `lattice.yaml` subgraph block ready to paste under `subgraphs:`.
#[derive(Serialize)]
struct SubgraphSnippet {
    schema: String,
    postgres: PostgresConnectorConfig,
}

/// Generate a subgraph (SDL + Postgres connector config) by introspecting a
/// database.
///
/// Connects to `url`, introspects the `public` schema, writes the GraphQL SDL to
/// `schema_out`, and prints a ready-to-paste `lattice.yaml` subgraph block.
///
/// # Errors
///
/// Returns [`PostgresCliError`] on connection, introspection, or write failures.
pub async fn run(url: &str, schema_out: &Path, name: &str) -> Result<(), PostgresCliError> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .map_err(|e| PostgresCliError::Connect(e.to_string()))?;
    let handle = tokio::spawn(async move {
        let _ = connection.await;
    });

    let tables = postgres::introspect(&client)
        .await
        .map_err(|e| PostgresCliError::Introspect(e.to_string()));
    handle.abort();
    let tables = tables?;

    let generated = postgres::generate(&tables, url);
    fs::write(schema_out, format!("{}\n", generated.sdl)).map_err(PostgresCliError::Write)?;

    let snippet = BTreeMap::from([(
        "subgraphs".to_string(),
        BTreeMap::from([(
            name.to_string(),
            SubgraphSnippet {
                schema: schema_out.display().to_string(),
                postgres: generated.connector,
            },
        )]),
    )]);

    // serde_yaml::to_string on an infallible, owned value does not fail here
    let yaml = serde_yaml::to_string(&snippet).unwrap_or_default();

    tracing::info!(
        schema = %schema_out.display(),
        tables = tables.len(),
        "generated subgraph from Postgres"
    );
    println!("# Add this subgraph to your lattice.yaml:\n{yaml}");

    Ok(())
}
