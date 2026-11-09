use std::{collections::BTreeMap, fs, io, path::Path};

use lattice_config::RestConnectorConfig;
use lattice_connectors::odata::{self, ODataError};
use serde::Serialize;
use thiserror::Error;

/// Errors raised while generating a subgraph from an OData metadata document.
#[derive(Debug, Error)]
pub enum ODataCliError {
    #[error("failed to read metadata document: {0}")]
    Read(#[source] io::Error),
    #[error("failed to generate subgraph: {0}")]
    Generate(#[from] ODataError),
    #[error("failed to write schema: {0}")]
    Write(#[source] io::Error),
}

/// A `lattice.yaml` subgraph block ready to paste under `subgraphs:`.
#[derive(Serialize)]
struct SubgraphSnippet {
    schema: String,
    rest: RestConnectorConfig,
}

/// Generate a subgraph (SDL + REST connector config) from an OData `$metadata`
/// (EDMX) document.
///
/// Writes the GraphQL SDL to `schema_out` and prints a ready-to-paste
/// `lattice.yaml` subgraph block to stdout. An OData service is plain HTTP+JSON,
/// so the generated config runs on the REST connector.
///
/// # Errors
///
/// Returns [`ODataCliError`] on read, generation, or write failures.
pub fn run(metadata_path: &Path, schema_out: &Path, name: &str, base_url: &str) -> Result<(), ODataCliError> {
    let edmx = fs::read_to_string(metadata_path).map_err(ODataCliError::Read)?;

    let generated = odata::generate(&edmx, base_url)?;
    let field_count = generated.connector.fields.len();

    fs::write(schema_out, format!("{}\n", generated.sdl)).map_err(ODataCliError::Write)?;

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
        fields = field_count,
        "generated subgraph from OData metadata"
    );
    println!("# Add this subgraph to your lattice.yaml:\n{yaml}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDMX: &str = r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
<edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="NS">
  <EntityType Name="Product">
    <Key><PropertyRef Name="ID"/></Key>
    <Property Name="ID" Type="Edm.Int32" Nullable="false"/>
    <Property Name="Name" Type="Edm.String"/>
  </EntityType>
  <EntityContainer Name="C"><EntitySet Name="Products" EntityType="NS.Product"/></EntityContainer>
</Schema></edmx:DataServices></edmx:Edmx>"#;

    #[test]
    fn run_writes_sdl_file_from_metadata() {
        let dir = std::env::temp_dir().join(format!("lattice-odata-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let meta = dir.join("metadata.xml");
        let out = dir.join("out.graphql");
        fs::write(&meta, EDMX).unwrap();

        run(&meta, &out, "northwind", "https://svc.example.com/odata").unwrap();

        let sdl = fs::read_to_string(&out).unwrap();
        assert!(sdl.contains("type Product {"), "sdl:\n{sdl}");
        assert!(sdl.contains("products: [Product!]"), "sdl:\n{sdl}");
        assert!(sdl.contains("product(ID: Int!): Product"), "sdl:\n{sdl}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_rejects_malformed_metadata() {
        let dir = std::env::temp_dir().join(format!("lattice-odata-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let meta = dir.join("bad.xml");
        let out = dir.join("out.graphql");
        fs::write(&meta, "<not valid <xml").unwrap();

        let err = run(&meta, &out, "svc", "http://x");
        assert!(matches!(err, Err(ODataCliError::Generate(_))), "got: {err:?}");

        fs::remove_dir_all(&dir).ok();
    }
}
