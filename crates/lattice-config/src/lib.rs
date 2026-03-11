mod env;
mod types;

pub use types::{Config, JwtConfig, PluginConfig, TelemetryConfig};

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse YAML config: {0}")]
    ParseYaml(#[from] serde_yaml::Error),
    #[error("failed to parse JSON config: {0}")]
    ParseJson(#[from] serde_json::Error),
}

/// Load config from a file, expanding `${VAR}` environment variables.
///
/// Supports YAML (`.yaml`, `.yml`) and JSON/JSONC (`.json`, `.jsonc`) formats.
/// JSONC files have line comments (`//`) and block comments (`/* */`) stripped
/// before parsing.
///
/// # Errors
///
/// Returns [`ConfigError::Read`] if the file cannot be read, or a parse error
/// if the content is invalid.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let raw = std::fs::read_to_string(path)?;
    let expanded = env::expand_vars(&raw);

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("yaml");

    match ext {
        "json" => Ok(serde_json::from_str(&expanded)?),
        "jsonc" => {
            let stripped = strip_json_comments(&expanded);
            Ok(serde_json::from_str(&stripped)?)
        }
        _ => Ok(serde_yaml::from_str(&expanded)?),
    }
}

/// Strip single-line (`//`) and block (`/* */`) comments from JSONC content.
fn strip_json_comments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if ch == '\\' {
                // Skip escaped character.
                if let Some(escaped) = chars.next() {
                    output.push(escaped);
                }
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }

        if ch == '/' {
            match chars.peek() {
                Some('/') => {
                    // Line comment — skip until newline.
                    for c in chars.by_ref() {
                        if c == '\n' {
                            output.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    // Block comment — skip until `*/`.
                    chars.next(); // consume '*'
                    while let Some(c) = chars.next() {
                        if c == '*' && chars.peek() == Some(&'/') {
                            chars.next(); // consume '/'
                            break;
                        }
                        // Preserve newlines for line numbering in error messages.
                        if c == '\n' {
                            output.push('\n');
                        }
                    }
                }
                _ => output.push(ch),
            }
        } else {
            output.push(ch);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
"#
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.server.listen, "0.0.0.0:4000");
        assert_eq!(config.supergraph.path.as_deref(), Some("./supergraph.graphql"));
    }

    #[test]
    fn load_json_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.json");
        std::fs::write(
            &path,
            r#"{
                "server": { "listen": "0.0.0.0:5000" },
                "supergraph": { "path": "./sg.graphql" }
            }"#,
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.server.listen, "0.0.0.0:5000");
    }

    #[test]
    fn load_jsonc_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.jsonc");
        std::fs::write(
            &path,
            r#"{
                // Server settings
                "server": { "listen": "0.0.0.0:5000" },
                /* Supergraph */
                "supergraph": { "path": "./sg.graphql" }
            }"#,
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.server.listen, "0.0.0.0:5000");
    }

    #[test]
    fn strip_comments_preserves_strings() {
        let input = r#"{"url": "http://example.com/path"}"#;
        assert_eq!(strip_json_comments(input), input);
    }

    #[test]
    fn strip_comments_removes_line_comments() {
        let input = "{\n// comment\n\"key\": \"value\"\n}";
        let stripped = strip_json_comments(input);
        assert!(!stripped.contains("comment"));
        assert!(stripped.contains("\"key\""));
    }

    #[test]
    fn strip_comments_removes_block_comments() {
        let input = r#"{"key": /* block */ "value"}"#;
        let stripped = strip_json_comments(input);
        assert!(!stripped.contains("block"));
        assert!(stripped.contains("\"value\""));
    }

    #[test]
    fn env_var_expansion() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "server:\n  listen: \"0.0.0.0:4000\"\nsupergraph:\n  path: ${{CARGO_MANIFEST_DIR}}/supergraph.graphql"
        )
        .unwrap();

        let config = load(&path).unwrap();
        assert_eq!(
            config.supergraph.path.as_deref(),
            Some(format!("{manifest_dir}/supergraph.graphql").as_str())
        );
    }

    #[test]
    fn config_with_subgraphs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
subgraphs:
  users:
    url: http://users:3000/graphql
  products:
    url: http://products:3001/graphql
"#
        )
        .unwrap();

        let config = load(&path).unwrap();
        let subgraphs = config.subgraphs.unwrap();
        assert_eq!(subgraphs.len(), 2);
        assert_eq!(subgraphs["users"].url.as_deref(), Some("http://users:3000/graphql"));
    }

    #[test]
    fn config_with_subgraph_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
subgraphs:
  users:
    url: http://users:3000/graphql
    headers:
      Authorization: "Bearer secret"
"#
        )
        .unwrap();

        let config = load(&path).unwrap();
        let subgraphs = config.subgraphs.unwrap();
        assert_eq!(
            subgraphs["users"].headers.get("Authorization").unwrap(),
            "Bearer secret"
        );
    }

    #[test]
    fn rejects_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        std::fs::write(&path, "not: [valid: yaml: {{{}}}").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn rejects_missing_file() {
        let path = Path::new("/nonexistent/lattice.yaml");
        assert!(load(path).is_err());
    }

    #[test]
    fn config_with_auth_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
auth:
  jwt:
    jwks_url: https://auth.example.com/.well-known/jwks.json
    issuer: https://auth.example.com
    audience: my-app
cache:
  entity:
    enabled: true
    valkey_url: redis://localhost:6379
    default_ttl: 60
rate_limit:
  enabled: true
  valkey_url: redis://localhost:6379
  default:
    requests: 1000
    window: 60s
telemetry:
  service_name: lattice-gateway
  otlp_endpoint: http://localhost:4317
  log_level: debug
  log_format: pretty
"#
        )
        .unwrap();

        let config = load(&path).unwrap();
        let auth = config.auth.unwrap();
        let jwt = auth.jwt.unwrap();
        assert_eq!(jwt.jwks_url, "https://auth.example.com/.well-known/jwks.json");
        assert_eq!(jwt.issuer.as_deref(), Some("https://auth.example.com"));
        assert!(jwt.forward_claims);

        let cache = config.cache.unwrap().entity.unwrap();
        assert!(cache.enabled);
        assert_eq!(cache.default_ttl, 60);

        let rl = config.rate_limit.unwrap();
        assert!(rl.enabled);
        assert_eq!(rl.default.unwrap().requests, 1000);

        let tel = config.telemetry.unwrap();
        assert_eq!(tel.service_name.as_deref(), Some("lattice-gateway"));
        assert_eq!(tel.log_format, "pretty");
    }

    #[test]
    fn config_with_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lattice.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
plugins:
  - name: my-auth
    path: ./plugins/auth.wasm
    hooks:
      - on_request
    config:
      allowed_roles: admin,user
"#
        )
        .unwrap();

        let config = load(&path).unwrap();
        let plugins = config.plugins.unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "my-auth");
        assert_eq!(plugins[0].hooks, vec!["on_request"]);
        assert!(plugins[0].config.contains_key("allowed_roles"));
    }
}
