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
    /// Returns an error if the file cannot be read, parsed, or contains invalid subgraph configs.
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let format = Self::detect_format(path)?;

        let content = fs::read_to_string(path)
            .map_err(|e| CliError::ReadComposeConfig(path.to_path_buf(), e))?;

        // Expand environment variables.
        let content = expand_env_vars(&content)?;

        let config: Self = match format {
            ConfigFormat::Json => serde_json::from_str(&content)
                .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))?,
            ConfigFormat::Jsonc => {
                let stripped = strip_jsonc_comments(&content);
                serde_json::from_str(&stripped)
                    .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))?
            }
            ConfigFormat::Yaml => serde_yaml::from_str(&content)
                .map_err(|e| CliError::ParseComposeConfig(path.to_path_buf(), e.to_string()))?,
        };

        config.validate()?;
        Ok(config)
    }

    /// Validate that all subgraph configs are valid.
    ///
    /// Each subgraph must have either `url` or both `schema` and `routing_url`.
    fn validate(&self) -> Result<(), CliError> {
        for (name, subgraph) in &self.subgraphs {
            let has_url = subgraph.url.is_some();
            let has_schema_and_routing = subgraph.schema.is_some() && subgraph.routing_url.is_some();

            if !has_url && !has_schema_and_routing {
                return Err(CliError::InvalidSubgraphConfig(name.clone()));
            }
        }
        Ok(())
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
    /// This is safe to call on configs loaded via `ComposeConfig::load()`, which
    /// validates that each subgraph has a valid URL configuration.
    pub fn routing_url(&self) -> &str {
        self.routing_url
            .as_deref()
            .or(self.url.as_deref())
            .expect("ComposeConfig::load validates url or routing_url exists")
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
                unreachable!("ComposeConfig::load validates url or schema exists")
            }
        }
    }
}

/// Expand environment variables in the format `${VAR_NAME}`.
fn expand_env_vars(content: &str) -> Result<String, CliError> {
    let mut result = content.to_string();
    // This pattern is always valid, so unwrap is safe.
    let re = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap();

    for cap in re.captures_iter(content) {
        let var_name = &cap[1];
        let var_value = std::env::var(var_name)
            .map_err(|_| CliError::EnvVarNotSet(var_name.to_string()))?;
        result = result.replace(&cap[0], &var_value);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    mod strip_jsonc_comments {
        use super::*;

        #[test]
        fn no_comments() {
            let input = r#"{"key": "value"}"#;
            assert_eq!(strip_jsonc_comments(input), input);
        }

        #[test]
        fn single_line_comment() {
            let input = r#"{
                // this is a comment
                "key": "value"
            }"#;
            // Whitespace before the comment is preserved.
            let expected = "{\n                \n                \"key\": \"value\"\n            }";
            assert_eq!(strip_jsonc_comments(input), expected);
        }

        #[test]
        fn multi_line_comment() {
            let input = r#"{
                /* this is a
                   multi-line comment */
                "key": "value"
            }"#;
            // Whitespace before and after the comment is preserved.
            let expected = "{\n                \n                \"key\": \"value\"\n            }";
            assert_eq!(strip_jsonc_comments(input), expected);
        }

        #[test]
        fn comment_like_string_preserved() {
            let input = r#"{"url": "http://example.com"}"#;
            assert_eq!(strip_jsonc_comments(input), input);
        }

        #[test]
        fn slash_in_string_preserved() {
            let input = r#"{"path": "foo/bar"}"#;
            assert_eq!(strip_jsonc_comments(input), input);
        }

        #[test]
        fn escaped_quote_in_string() {
            let input = r#"{"key": "value with \" quote"}"#;
            assert_eq!(strip_jsonc_comments(input), input);
        }

        #[test]
        fn inline_comment_after_value() {
            let input = r#"{"key": "value" // comment
            }"#;
            // The space before the comment is preserved.
            let expected = "{\"key\": \"value\" \n            }";
            assert_eq!(strip_jsonc_comments(input), expected);
        }
    }

    mod expand_env_vars {
        use super::*;

        #[test]
        fn no_vars() {
            let input = r#"{"key": "value"}"#;
            assert_eq!(expand_env_vars(input).unwrap(), input);
        }

        #[test]
        fn single_var() {
            std::env::set_var("TEST_COMPOSE_VAR", "expanded");
            let input = r#"{"url": "${TEST_COMPOSE_VAR}"}"#;
            let expected = r#"{"url": "expanded"}"#;
            assert_eq!(expand_env_vars(input).unwrap(), expected);
            std::env::remove_var("TEST_COMPOSE_VAR");
        }

        #[test]
        fn multiple_vars() {
            std::env::set_var("TEST_HOST", "localhost");
            std::env::set_var("TEST_PORT", "4000");
            let input = r#"{"url": "http://${TEST_HOST}:${TEST_PORT}"}"#;
            let expected = r#"{"url": "http://localhost:4000"}"#;
            assert_eq!(expand_env_vars(input).unwrap(), expected);
            std::env::remove_var("TEST_HOST");
            std::env::remove_var("TEST_PORT");
        }

        #[test]
        fn missing_var_returns_error() {
            let input = r#"{"url": "${NONEXISTENT_VAR_12345}"}"#;
            let result = expand_env_vars(input);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                CliError::EnvVarNotSet(name) if name == "NONEXISTENT_VAR_12345"
            ));
        }

        #[test]
        fn var_with_underscore() {
            std::env::set_var("TEST_VAR_WITH_UNDERSCORE", "works");
            let input = r#"${TEST_VAR_WITH_UNDERSCORE}"#;
            assert_eq!(expand_env_vars(input).unwrap(), "works");
            std::env::remove_var("TEST_VAR_WITH_UNDERSCORE");
        }

        #[test]
        fn lowercase_not_matched() {
            let input = r#"${lowercase}"#;
            // Lowercase vars are not matched by the pattern.
            assert_eq!(expand_env_vars(input).unwrap(), input);
        }
    }

    mod detect_format {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn json_extension() {
            let path = PathBuf::from("config.json");
            assert!(matches!(
                ComposeConfig::detect_format(&path).unwrap(),
                ConfigFormat::Json
            ));
        }

        #[test]
        fn jsonc_extension() {
            let path = PathBuf::from("config.jsonc");
            assert!(matches!(
                ComposeConfig::detect_format(&path).unwrap(),
                ConfigFormat::Jsonc
            ));
        }

        #[test]
        fn yaml_extension() {
            let path = PathBuf::from("config.yaml");
            assert!(matches!(
                ComposeConfig::detect_format(&path).unwrap(),
                ConfigFormat::Yaml
            ));
        }

        #[test]
        fn yml_extension() {
            let path = PathBuf::from("config.yml");
            assert!(matches!(
                ComposeConfig::detect_format(&path).unwrap(),
                ConfigFormat::Yaml
            ));
        }

        #[test]
        fn uppercase_extension() {
            let path = PathBuf::from("config.JSON");
            assert!(matches!(
                ComposeConfig::detect_format(&path).unwrap(),
                ConfigFormat::Json
            ));
        }

        #[test]
        fn unsupported_extension() {
            let path = PathBuf::from("config.toml");
            assert!(ComposeConfig::detect_format(&path).is_err());
        }

        #[test]
        fn no_extension() {
            let path = PathBuf::from("config");
            assert!(ComposeConfig::detect_format(&path).is_err());
        }
    }

    mod load_config {
        use super::*;

        fn write_temp_config(content: &str, extension: &str) -> NamedTempFile {
            let mut file = tempfile::Builder::new()
                .suffix(extension)
                .tempfile()
                .unwrap();
            file.write_all(content.as_bytes()).unwrap();
            file
        }

        #[test]
        fn load_json() {
            let content = r#"{
                "subgraphs": {
                    "users": { "url": "http://localhost:4001/graphql" }
                }
            }"#;
            let file = write_temp_config(content, ".json");
            let config = ComposeConfig::load(file.path()).unwrap();
            assert_eq!(config.subgraphs.len(), 1);
            assert!(config.subgraphs.contains_key("users"));
        }

        #[test]
        fn load_yaml() {
            let content = r#"
subgraphs:
  users:
    url: http://localhost:4001/graphql
"#;
            let file = write_temp_config(content, ".yaml");
            let config = ComposeConfig::load(file.path()).unwrap();
            assert_eq!(config.subgraphs.len(), 1);
            assert!(config.subgraphs.contains_key("users"));
        }

        #[test]
        fn load_jsonc_with_comments() {
            let content = r#"{
                // Subgraph configuration
                "subgraphs": {
                    "users": { "url": "http://localhost:4001/graphql" }
                }
            }"#;
            let file = write_temp_config(content, ".jsonc");
            let config = ComposeConfig::load(file.path()).unwrap();
            assert_eq!(config.subgraphs.len(), 1);
        }

        #[test]
        fn load_with_headers() {
            let content = r#"{
                "subgraphs": {
                    "users": { "url": "http://localhost:4001/graphql" }
                },
                "headers": {
                    "Authorization": "Bearer token"
                }
            }"#;
            let file = write_temp_config(content, ".json");
            let config = ComposeConfig::load(file.path()).unwrap();
            assert_eq!(config.headers.get("Authorization").unwrap(), "Bearer token");
        }

        #[test]
        fn load_with_schema_and_routing_url() {
            let content = r#"{
                "subgraphs": {
                    "users": {
                        "schema": "./users.graphql",
                        "routing_url": "http://localhost:4001/graphql"
                    }
                }
            }"#;
            let file = write_temp_config(content, ".json");
            let config = ComposeConfig::load(file.path()).unwrap();
            let users = config.subgraphs.get("users").unwrap();
            assert_eq!(users.schema.as_deref(), Some("./users.graphql"));
            assert_eq!(users.routing_url.as_deref(), Some("http://localhost:4001/graphql"));
        }

        #[test]
        fn load_nonexistent_file() {
            let path = Path::new("/nonexistent/path/config.json");
            assert!(ComposeConfig::load(path).is_err());
        }

        #[test]
        fn load_invalid_json() {
            let content = r#"{ invalid json }"#;
            let file = write_temp_config(content, ".json");
            assert!(ComposeConfig::load(file.path()).is_err());
        }
    }

    mod validation {
        use super::*;

        fn write_temp_config(content: &str) -> NamedTempFile {
            let mut file = tempfile::Builder::new()
                .suffix(".json")
                .tempfile()
                .unwrap();
            file.write_all(content.as_bytes()).unwrap();
            file
        }

        #[test]
        fn valid_with_url() {
            let content = r#"{
                "subgraphs": {
                    "users": { "url": "http://localhost:4001/graphql" }
                }
            }"#;
            let file = write_temp_config(content);
            assert!(ComposeConfig::load(file.path()).is_ok());
        }

        #[test]
        fn valid_with_schema_and_routing_url() {
            let content = r#"{
                "subgraphs": {
                    "users": {
                        "schema": "./users.graphql",
                        "routing_url": "http://localhost:4001/graphql"
                    }
                }
            }"#;
            let file = write_temp_config(content);
            assert!(ComposeConfig::load(file.path()).is_ok());
        }

        #[test]
        fn valid_with_url_and_schema() {
            // Having url + schema is valid (schema takes precedence).
            let content = r#"{
                "subgraphs": {
                    "users": {
                        "url": "http://localhost:4001/graphql",
                        "schema": "./users.graphql"
                    }
                }
            }"#;
            let file = write_temp_config(content);
            assert!(ComposeConfig::load(file.path()).is_ok());
        }

        #[test]
        fn invalid_no_url_or_schema() {
            let content = r#"{
                "subgraphs": {
                    "users": {}
                }
            }"#;
            let file = write_temp_config(content);
            let result = ComposeConfig::load(file.path());
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                CliError::InvalidSubgraphConfig(name) if name == "users"
            ));
        }

        #[test]
        fn invalid_schema_without_routing_url() {
            let content = r#"{
                "subgraphs": {
                    "users": { "schema": "./users.graphql" }
                }
            }"#;
            let file = write_temp_config(content);
            let result = ComposeConfig::load(file.path());
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                CliError::InvalidSubgraphConfig(name) if name == "users"
            ));
        }

        #[test]
        fn invalid_routing_url_without_schema_or_url() {
            let content = r#"{
                "subgraphs": {
                    "users": { "routing_url": "http://localhost:4001/graphql" }
                }
            }"#;
            let file = write_temp_config(content);
            let result = ComposeConfig::load(file.path());
            assert!(result.is_err());
        }
    }

    mod routing_url {
        use super::*;

        #[test]
        fn returns_routing_url_when_set() {
            let config = SubgraphConfig {
                url: Some("http://url".to_string()),
                schema: None,
                routing_url: Some("http://routing".to_string()),
            };
            assert_eq!(config.routing_url(), "http://routing");
        }

        #[test]
        fn falls_back_to_url() {
            let config = SubgraphConfig {
                url: Some("http://url".to_string()),
                schema: None,
                routing_url: None,
            };
            assert_eq!(config.routing_url(), "http://url");
        }
    }
}
