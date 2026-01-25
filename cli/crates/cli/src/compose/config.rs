use crate::errors::CliError;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

/// Configuration for composing subgraphs into a federated supergraph.
#[derive(Debug, Deserialize)]
pub struct ComposeConfig {
    /// Map of subgraph name to configuration.
    pub subgraphs: BTreeMap<String, SubgraphConfig>,
    /// Headers to send with introspection requests.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// Configuration for a single subgraph.
#[derive(Debug, Deserialize)]
pub struct SubgraphConfig {
    /// URL for introspection (also used as routing_url if schema is not provided).
    pub url: Option<String>,
    /// Path to local schema file.
    pub schema: Option<String>,
    /// Routing URL (required if only schema is provided).
    pub routing_url: Option<String>,
}

impl ComposeConfig {
    /// Load and parse compose config from a YAML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let content = fs::read_to_string(path)
            .map_err(|e| CliError::ReadComposeConfig(path.to_path_buf(), e))?;

        // Expand environment variables.
        let content = expand_env_vars(&content)?;

        serde_yaml::from_str(&content)
            .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))
    }
}

impl SubgraphConfig {
    /// Get the routing URL for this subgraph.
    ///
    /// Returns the explicit `routing_url` if set, otherwise falls back to `url`.
    ///
    /// # Panics
    ///
    /// Panics if neither `routing_url` nor `url` is set.
    pub fn routing_url(&self) -> &str {
        self.routing_url
            .as_deref()
            .or(self.url.as_deref())
            .expect("validation ensures url or routing_url exists")
    }

    /// Fetch the schema for this subgraph.
    ///
    /// If `schema` is set, reads from the local file.
    /// Otherwise, introspects from the `url`.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema file cannot be read or introspection fails.
    pub async fn fetch_schema(&self, headers: &BTreeMap<String, String>) -> Result<String, CliError> {
        match (&self.schema, &self.url) {
            (Some(path), _) => {
                // Read from file.
                let path = Path::new(path);
                fs::read_to_string(path)
                    .map_err(|e| CliError::ReadSubgraphSchema(path.to_path_buf(), e))
            }
            (None, Some(url)) => {
                // Introspect from URL.
                let header_pairs: Vec<_> = headers
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();

                grafbase_graphql_introspection::introspect(url, &header_pairs)
                    .await
                    .map_err(CliError::Introspection)
            }
            (None, None) => {
                unreachable!("validation ensures url or schema exists")
            }
        }
    }
}

/// Expand environment variables in the format `${VAR_NAME}`.
fn expand_env_vars(content: &str) -> Result<String, CliError> {
    let mut result = content.to_string();
    let re = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap();

    for cap in re.captures_iter(content) {
        let var_name = &cap[1];
        let var_value = std::env::var(var_name)
            .map_err(|_| CliError::EnvVarNotSet(var_name.to_string()))?;
        result = result.replace(&cap[0], &var_value);
    }

    Ok(result)
}
