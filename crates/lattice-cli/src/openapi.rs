use std::{collections::BTreeMap, fs, io, path::Path};

use lattice_config::RestConnectorConfig;
use lattice_connectors::openapi::{self, OpenApiError};
use serde::Serialize;
use thiserror::Error;

/// Errors raised while generating a subgraph from an OpenAPI spec.
#[derive(Debug, Error)]
pub enum OpenApiCliError {
    #[error("failed to read spec: {0}")]
    Read(#[source] io::Error),
    #[error("failed to parse spec (expected JSON or YAML): {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("failed to generate subgraph: {0}")]
    Generate(#[from] OpenApiError),
    #[error("failed to write schema: {0}")]
    Write(#[source] io::Error),
}

/// A `lattice.yaml` subgraph block ready to paste under `subgraphs:`.
#[derive(Serialize)]
struct SubgraphSnippet {
    schema: String,
    rest: RestConnectorConfig,
}

/// Generate a subgraph (SDL + REST connector config) from an OpenAPI spec.
///
/// Writes the GraphQL SDL to `schema_out` and prints a ready-to-paste
/// `lattice.yaml` subgraph block to stdout.
///
/// # Errors
///
/// Returns [`OpenApiCliError`] on read, parse, generation, or write failures.
pub fn run(spec_path: &Path, schema_out: &Path, name: &str, base_url: Option<&str>) -> Result<(), OpenApiCliError> {
    let text = fs::read_to_string(spec_path).map_err(OpenApiCliError::Read)?;
    // serde_yaml parses JSON too (JSON is a YAML subset), so one path covers
    // both `.json` and `.yaml` specs
    let spec: serde_json::Value = serde_yaml::from_str(&text)?;

    let mut generated = openapi::generate(&spec)?;
    if let Some(url) = base_url {
        generated.connector.base_url = url.to_string();
    }

    fs::write(schema_out, format!("{}\n", generated.sdl)).map_err(OpenApiCliError::Write)?;

    let snippet = BTreeMap::from([(
        "subgraphs".to_string(),
        BTreeMap::from([(
            name.to_string(),
            SubgraphSnippet {
                schema: schema_out.display().to_string(),
                rest: generated.connector,
            },
        )]),
    )]);

    // serde_yaml::to_string on an infallible, owned value does not fail here
    let yaml = serde_yaml::to_string(&snippet).unwrap_or_default();

    tracing::info!(
        schema = %schema_out.display(),
        fields = generated_field_count(&yaml),
        "generated subgraph from OpenAPI spec"
    );
    println!("# Add this subgraph to your lattice.yaml:\n{yaml}");

    Ok(())
}

/// Count the mapped fields for the log line (cheap: counts `path:` keys).
fn generated_field_count(yaml: &str) -> usize {
    yaml.lines().filter(|l| l.trim_start().starts_with("path:")).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_writes_sdl_file_from_spec() {
        let dir = std::env::temp_dir().join(format!("lattice-openapi-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let spec = dir.join("spec.json");
        let out = dir.join("out.graphql");
        fs::write(
            &spec,
            r#"{"servers":[{"url":"https://x.test"}],
                "paths":{"/pets":{"get":{"operationId":"listPets",
                "responses":{"200":{"content":{"application/json":{
                "schema":{"type":"array","items":{"type":"string"}}}}}}}}}}"#,
        )
        .unwrap();

        run(&spec, &out, "petstore", None).unwrap();

        let sdl = fs::read_to_string(&out).unwrap();
        assert!(sdl.contains("type Query {"), "sdl:\n{sdl}");
        assert!(sdl.contains("listPets: [String]"), "sdl:\n{sdl}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_applies_base_url_override() {
        let dir = std::env::temp_dir().join(format!("lattice-openapi-ov-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let spec = dir.join("spec.json");
        let out = dir.join("out.graphql");
        // spec has no servers; the override must supply the base URL
        fs::write(
            &spec,
            r#"{"paths":{"/ping":{"get":{"operationId":"ping",
                "responses":{"200":{"content":{"application/json":{
                "schema":{"type":"string"}}}}}}}}}"#,
        )
        .unwrap();

        // run succeeds and writes SDL; the override lands in the printed snippet
        run(&spec, &out, "svc", Some("https://override.test")).unwrap();
        assert!(fs::read_to_string(&out).unwrap().contains("ping: String"));

        fs::remove_dir_all(&dir).ok();
    }
}
