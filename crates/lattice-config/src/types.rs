use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub supergraph: SupergraphConfig,
    #[serde(default)]
    pub subgraphs: Option<HashMap<String, SubgraphConfig>>,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub cache: Option<CacheConfig>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
    #[serde(default)]
    pub plugins: Option<Vec<PluginConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub playground: Option<bool>,
}

fn default_listen() -> String {
    "0.0.0.0:4000".into()
}

#[derive(Debug, Deserialize)]
pub struct SupergraphConfig {
    pub path: Option<String>,
    pub url: Option<String>,
    pub poll_interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubgraphConfig {
    pub url: Option<String>,
    pub schema: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    pub jwt: Option<JwtConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtConfig {
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub jwks_url: String,
    #[serde(default = "default_true")]
    pub forward_claims: bool,
    #[serde(default = "default_auth_header")]
    pub header_name: String,
}

const fn default_true() -> bool {
    true
}

fn default_auth_header() -> String {
    "Authorization".into()
}

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub entity: Option<EntityCacheConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntityCacheConfig {
    #[serde(default)]
    pub enabled: bool,
    pub valkey_url: String,
    #[serde(default = "default_ttl")]
    pub default_ttl: u64,
    #[serde(default = "default_cache_prefix")]
    pub key_prefix: String,
}

const fn default_ttl() -> u64 {
    300
}

fn default_cache_prefix() -> String {
    "lattice:".into()
}

#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default)]
    pub enabled: bool,
    pub valkey_url: String,
    pub default: Option<RateLimitRule>,
    #[serde(default)]
    pub overrides: Vec<RateLimitOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitRule {
    pub requests: u64,
    pub window: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitOverride {
    pub operations: Vec<String>,
    pub requests: u64,
    pub window: String,
}

#[derive(Debug, Deserialize)]
pub struct TelemetryConfig {
    pub service_name: Option<String>,
    pub otlp_endpoint: Option<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
}

fn default_log_level() -> String {
    "info".into()
}

fn default_log_format() -> String {
    "json".into()
}

#[derive(Debug, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub config: HashMap<String, serde_yaml::Value>,
}
