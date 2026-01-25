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

/// Supported config file formats.
#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    Json,
    Jsonc,
    Yaml,
}

impl ComposeConfig {
    /// Load and parse compose config from a file.
    ///
    /// Supports JSON (.json), JSONC (.jsonc), and YAML (.yaml, .yml) formats.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let format = Self::detect_format(path)?;

        let content = fs::read_to_string(path)
            .map_err(|e| CliError::ReadComposeConfig(path.to_path_buf(), e))?;

        // Expand environment variables.
        let content = expand_env_vars(&content)?;

        match format {
            ConfigFormat::Json => serde_json::from_str(&content)
                .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string())),
            ConfigFormat::Jsonc => {
                let stripped = strip_jsonc_comments(&content);
                serde_json::from_str(&stripped)
                    .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))
            }
            ConfigFormat::Yaml => serde_yaml::from_str(&content)
                .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string())),
        }
    }

    /// Detect the config format from the file extension.
    fn detect_format(path: &Path) -> Result<ConfigFormat, CliError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        match ext.as_deref() {
            Some("json") => Ok(ConfigFormat::Json),
            Some("jsonc") => Ok(ConfigFormat::Jsonc),
            Some("yaml" | "yml") => Ok(ConfigFormat::Yaml),
            _ => Err(CliError::ParseComposeConfig(
                path.to_path_buf(),
                "unsupported file extension (use .json, .jsonc, .yaml, or .yml)".to_string(),
            )),
        }
    }
}

/// Strip single-line (//) and multi-line (/* */) comments from JSONC content.
fn strip_jsonc_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }

        if c == '"' && !escape_next {
            in_string = !in_string;
            result.push(c);
            continue;
        }

        if !in_string && c == '/' {
            if let Some(&next) = chars.peek() {
                if next == '/' {
                    // Single-line comment - skip to end of line.
                    chars.next();
                    while let Some(&ch) = chars.peek() {
                        if ch == '\n' {
                            break;
                        }
                        chars.next();
                    }
                    continue;
                } else if next == '*' {
                    // Multi-line comment - skip to */.
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '*' {
                            if let Some(&'/') = chars.peek() {
                                chars.next();
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        result.push(c);
    }

    result
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
