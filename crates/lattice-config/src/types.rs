use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub mcp: Option<McpConfig>,
    #[serde(default)]
    pub contracts: Option<ContractsConfig>,
    #[serde(default)]
    pub cors: Option<CorsConfig>,
    #[serde(default)]
    pub csrf: Option<CsrfConfig>,
    #[serde(default)]
    pub limits: Option<LimitsConfig>,
    #[serde(default)]
    pub traffic_shaping: Option<TrafficShapingConfig>,
    #[serde(default)]
    pub authorization: Option<AuthorizationConfig>,
    #[serde(default)]
    pub persisted_queries: Option<PersistedQueriesConfig>,
}

/// Automatic Persisted Queries (APQ).
///
/// When enabled, clients may send an operation's SHA-256 hash in place of the
/// full query body to save bandwidth. The gateway recovers the registered query
/// from the hash, or asks the client to retry with the body on a cache miss.
#[derive(Debug, Deserialize)]
pub struct PersistedQueriesConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Maximum number of registered operations held in memory before the oldest
    /// are evicted.
    #[serde(default = "default_apq_cache_size")]
    pub cache_size: usize,
}

const fn default_apq_cache_size() -> usize {
    1000
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
}

#[derive(Debug, Deserialize)]
pub struct SubgraphConfig {
    pub url: Option<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// REST connector. When set, the subgraph is backed by a REST upstream
    /// instead of a GraphQL `url`, and the gateway translates its subqueries
    /// into HTTP calls.
    #[serde(default)]
    pub rest: Option<RestConnectorConfig>,
}

/// A REST upstream exposed to the federated graph as a subgraph. The supergraph
/// SDL declares the subgraph's shape; this config maps its root fields to HTTP
/// calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestConnectorConfig {
    /// Base URL prepended to every field's `path`.
    pub base_url: String,
    /// Static headers sent on every request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Map of GraphQL root field name to its REST operation.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, RestFieldMapping>,
    /// Entity resolvers keyed by GraphQL type name, used to resolve federated
    /// `@key` references (`_entities`) over REST. The representation's key
    /// fields fill the mapping's `path` placeholders.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub entities: HashMap<String, RestFieldMapping>,
}

/// How one GraphQL root field maps to a REST request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestFieldMapping {
    /// HTTP method (defaults to `GET`).
    #[serde(default = "default_http_method")]
    pub method: String,
    /// Path template appended to the connector `base_url`. `{arg}` placeholders
    /// are filled from the field's arguments.
    pub path: String,
    /// Query-string parameters, mapping a parameter name to a field argument
    /// name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub query: HashMap<String, String>,
    /// Optional JSON Pointer (RFC 6901) selecting a sub-value of the response
    /// body as the field result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<String>,
    /// For write methods, the field argument whose value is sent as the JSON
    /// request body (for example `input` on a `createPet(input: ...)` mutation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Response field remapping for upstreams whose JSON keys differ from the
    /// GraphQL field names, mapping a GraphQL field name to the source key or
    /// JSON Pointer in the (post-`select`) response. Unmapped keys pass through.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub response: HashMap<String, String>,
}

fn default_http_method() -> String {
    "GET".to_string()
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    /// Legacy single JWT provider (backward compat).
    pub jwt: Option<JwtConfig>,
    /// Default behavior when no provider succeeds: "deny" or "anonymous".
    #[serde(default)]
    pub default: Option<String>,
    /// JWT/JWKS providers, evaluated in order.
    #[serde(default)]
    pub providers: Option<Vec<JwtProviderConfig>>,
    /// Static API key authentication.
    #[serde(default)]
    pub api_keys: Option<ApiKeysConfig>,
    /// Per-resource auth overrides.
    #[serde(default)]
    pub resources: Option<ResourceAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtConfig {
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub jwks_url: String,
    #[serde(default = "default_auth_header")]
    pub header_name: String,
}

fn default_auth_header() -> String {
    "Authorization".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtProviderConfig {
    pub jwks_url: String,
    #[serde(default)]
    pub issuer: Option<String>,
    /// Single string or array of strings.
    #[serde(default)]
    pub audience: Option<serde_json::Value>,
    #[serde(default = "default_auth_header")]
    pub header_name: String,
    /// Header value prefix (default: "Bearer ").
    #[serde(default)]
    pub header_prefix: Option<String>,
    /// JWKS polling interval in seconds.
    #[serde(default)]
    pub poll_interval: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeysConfig {
    #[serde(default = "default_api_key_header")]
    pub header_name: String,
    pub keys: Vec<ApiKeyEntry>,
}

fn default_api_key_header() -> String {
    "X-API-Key".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyEntry {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub claims: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceAuthConfig {
    #[serde(default)]
    pub graphql: Option<ResourceAuthOverride>,
    #[serde(default)]
    pub mcp: Option<ResourceAuthOverride>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceAuthOverride {
    #[serde(default)]
    pub default: Option<String>,
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
    #[serde(default)]
    pub prometheus: Option<PrometheusConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrometheusConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_metrics_path")]
    pub path: String,
}

fn default_metrics_path() -> String {
    "/metrics".into()
}

fn default_log_level() -> String {
    "info".into()
}

fn default_log_format() -> String {
    "json".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mcp_path")]
    pub path: String,
}

fn default_mcp_path() -> String {
    "/mcp".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractsConfig {
    #[serde(default = "default_contract_key")]
    pub default: String,
    #[serde(default = "default_contract_header")]
    pub header_name: String,
    pub variants: HashMap<String, ContractVariantConfig>,
}

fn default_contract_key() -> String {
    "full".into()
}

fn default_contract_header() -> String {
    "X-Contract-Key".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractVariantConfig {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub origins: Vec<String>,
    #[serde(default = "default_cors_methods")]
    pub methods: Vec<String>,
    #[serde(default)]
    pub allow_headers: Vec<String>,
    #[serde(default)]
    pub allow_credentials: bool,
    #[serde(default = "default_cors_max_age")]
    pub max_age: u64,
}

fn default_cors_methods() -> Vec<String> {
    vec!["GET".into(), "POST".into(), "OPTIONS".into()]
}

const fn default_cors_max_age() -> u64 {
    86400
}

#[derive(Debug, Clone, Deserialize)]
pub struct CsrfConfig {
    #[serde(default = "default_csrf_headers")]
    pub required_headers: Vec<String>,
}

fn default_csrf_headers() -> Vec<String> {
    vec!["x-graphql-csrf".into()]
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_max_complexity")]
    pub max_complexity: usize,
    #[serde(default = "default_max_body_size")]
    pub max_body_size: String,
    #[serde(default = "default_max_aliases")]
    pub max_aliases: usize,
}

const fn default_max_depth() -> usize {
    15
}

const fn default_max_complexity() -> usize {
    1000
}

fn default_max_body_size() -> String {
    "2mb".into()
}

const fn default_max_aliases() -> usize {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizationConfig {
    /// "filter" (null unauthorized fields) or "reject" (return error).
    #[serde(default = "default_authz_mode")]
    pub mode: String,
    #[serde(default)]
    pub directives: AuthorizationDirectivesConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuthorizationDirectivesConfig {
    /// Enforce `@requiresScopes` directives.
    #[serde(default)]
    pub requires_scopes: bool,
    /// Enforce `@requiresAuthentication` directives.
    #[serde(default)]
    pub requires_authentication: bool,
}

fn default_authz_mode() -> String {
    "filter".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrafficShapingConfig {
    #[serde(default)]
    pub dedupe_enabled: bool,
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
