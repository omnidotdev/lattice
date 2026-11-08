use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use lattice_cache::ValkeyCache;
use lattice_config::Config;
use lattice_execution::SubgraphClient;
use lattice_federation::FederationPlanner;
use lattice_plugin::PluginManager;
use lattice_rate_limit::{RateLimiter, ValkeyRateLimiter};

/// Atomically swappable shared state for hot-reload support.
pub type SharedState = Arc<ArcSwap<AppState>>;

/// Shared application state for the axum server.
pub struct AppState {
    pub config: Config,
    pub planner: Option<FederationPlanner>,
    pub subgraph_client: SubgraphClient,
    /// Map of subgraph name to URL (from config).
    pub subgraph_urls: HashMap<String, String>,
    /// JWT validators, one per configured provider.
    pub jwt_validators: Vec<lattice_auth::JwtValidator>,
    /// Optional API key validator.
    pub api_key_validator: Option<lattice_auth::ApiKeyValidator>,
    /// Auth default behavior: "deny" or "anonymous".
    pub auth_default: Option<String>,
    /// Per-resource auth overrides.
    pub auth_resources: Option<lattice_config::ResourceAuthConfig>,
    /// JWKS polling task handles (kept alive for the server's lifetime).
    pub poll_handles: Vec<tokio::task::JoinHandle<()>>,
    /// Optional rate limiter backend.
    pub rate_limiter: Option<Arc<dyn RateLimiter>>,
    /// Default rate limit parameters (`max_requests`, `window_seconds`).
    pub rate_limit_defaults: Option<(u64, u64)>,
    /// Optional WASI plugin manager.
    pub plugin_manager: Option<PluginManager>,
    /// Optional entity/response cache.
    pub entity_cache: Option<ValkeyCache>,
    /// Cache TTL in seconds.
    pub cache_ttl: u64,
    /// Raw supergraph SDL, retained for MCP introspection.
    pub supergraph_sdl: Option<String>,
    /// Optional contract registry for schema filtering.
    pub contract_registry: Option<crate::contracts::ContractRegistry>,
    /// In-process subgraph executors (e.g. taproot)
    pub in_process_subgraphs: HashMap<String, Arc<dyn lattice_execution::SubgraphExecutor>>,
    /// Optional request deduplication layer.
    pub dedup_layer: Option<lattice_execution::DedupLayer>,
    /// Directive-based authorization tree.
    pub auth_tree: Option<lattice_auth::AuthTree>,
    /// Authorization enforcement mode ("filter" or "reject").
    pub authz_mode: Option<String>,
    /// Automatic Persisted Queries store, present when APQ is enabled in config.
    pub apq_store: Option<crate::apq::ApqStore>,
}

impl AppState {
    /// Create state from a loaded config and optional supergraph SDL.
    #[must_use]
    pub fn new(config: Config, supergraph_sdl: Option<&str>) -> Self {
        let planner = supergraph_sdl.and_then(|sdl| match FederationPlanner::from_sdl(sdl) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!("failed to load supergraph: {e}");
                None
            }
        });

        let subgraph_urls = config
            .subgraphs
            .as_ref()
            .map(|sgs| {
                sgs.iter()
                    .filter_map(|(name, sg)| sg.url.as_ref().map(|url| (name.clone(), url.clone())))
                    .collect()
            })
            .unwrap_or_default();

        // Subgraphs backed by a REST connector are served in-process: the
        // planner routes fetches to them like any subgraph, and the connector
        // translates each subquery into HTTP calls.
        let in_process_subgraphs: HashMap<String, Arc<dyn lattice_execution::SubgraphExecutor>> = config
            .subgraphs
            .as_ref()
            .map(|sgs| {
                sgs.iter()
                    .filter_map(|(name, sg)| {
                        sg.rest.as_ref().map(|rest| {
                            tracing::info!(subgraph = %name, "registered REST connector subgraph");
                            let executor: Arc<dyn lattice_execution::SubgraphExecutor> =
                                Arc::new(lattice_connectors::rest::RestSubgraphExecutor::from_config(rest));
                            (name.clone(), executor)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let (jwt_validators, api_key_validator, auth_default, auth_resources, poll_handles) = init_auth(&config);
        let (rate_limiter, rate_limit_defaults) = init_rate_limiter(&config);
        let (entity_cache, cache_ttl) = init_cache(&config);

        let dedup_layer = config.traffic_shaping.as_ref().filter(|ts| ts.dedupe_enabled).map(|_| {
            tracing::info!("request deduplication enabled");
            lattice_execution::DedupLayer::new()
        });

        let (auth_tree, authz_mode) = config.authorization.as_ref().map_or((None, None), |authz| {
            let tree = supergraph_sdl
                .map(|sdl| {
                    lattice_auth::build_auth_tree(
                        sdl,
                        authz.directives.requires_scopes,
                        authz.directives.requires_authentication,
                    )
                })
                .filter(|t| !t.is_empty());

            if tree.is_some() {
                tracing::info!(mode = %authz.mode, "directive-based authorization enabled");
            }

            (tree, Some(authz.mode.clone()))
        });

        let contract_registry = config.contracts.as_ref().and_then(|contracts_config| {
            supergraph_sdl.and_then(
                |sdl| match crate::contracts::ContractRegistry::build(contracts_config, sdl) {
                    Ok(registry) => {
                        tracing::info!(variants = registry.variant_count(), "contract registry built");
                        Some(registry)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "failed to build contract registry");
                        None
                    }
                },
            )
        });

        let apq_store = config.persisted_queries.as_ref().filter(|pq| pq.enabled).map(|pq| {
            tracing::info!(cache_size = pq.cache_size, "automatic persisted queries enabled");
            crate::apq::ApqStore::new(pq.cache_size)
        });

        Self {
            config,
            planner,
            subgraph_client: SubgraphClient::new(),
            subgraph_urls,
            jwt_validators,
            api_key_validator,
            auth_default,
            auth_resources,
            poll_handles,
            rate_limiter,
            rate_limit_defaults,
            plugin_manager: None,
            entity_cache,
            cache_ttl,
            supergraph_sdl: supergraph_sdl.map(String::from),
            contract_registry,
            in_process_subgraphs,
            dedup_layer,
            auth_tree,
            authz_mode,
            apq_store,
        }
    }

    /// Register an in-process subgraph executor.
    pub fn register_executor(&mut self, name: String, executor: Arc<dyn lattice_execution::SubgraphExecutor>) {
        self.in_process_subgraphs.insert(name, executor);
    }
}

type AuthInit = (
    Vec<lattice_auth::JwtValidator>,
    Option<lattice_auth::ApiKeyValidator>,
    Option<String>,
    Option<lattice_config::ResourceAuthConfig>,
    Vec<tokio::task::JoinHandle<()>>,
);

/// Initialize auth validators from config.
fn init_auth(config: &Config) -> AuthInit {
    let Some(auth) = &config.auth else {
        return (vec![], None, None, None, vec![]);
    };

    let mut validators = Vec::new();
    let mut poll_handles = Vec::new();

    // New multi-provider config.
    if let Some(providers) = &auth.providers {
        for p in providers {
            let provider = lattice_auth::JwksProvider::new(&p.jwks_url);

            if let Some(interval) = p.poll_interval {
                poll_handles.push(provider.start_polling(interval));
            }

            let mut validator = lattice_auth::JwtValidator::new(provider, p.issuer.clone(), None);

            if let Some(aud_value) = &p.audience {
                let audiences = lattice_auth::parse_audience(aud_value);
                validator = validator.with_audiences(audiences);
            }

            if let Some(prefix) = &p.header_prefix {
                validator = validator.with_header_prefix(prefix);
            }

            validators.push(validator);
        }
    }

    // Legacy single-provider (backward compat).
    if validators.is_empty() {
        if let Some(jwt) = &auth.jwt {
            validators.push(lattice_auth::JwtValidator::from_config(jwt));
        }
    }

    // API key validator.
    let api_key_validator = auth.api_keys.as_ref().map(|ak| {
        let keys: Vec<(&str, &str, std::collections::HashMap<String, serde_json::Value>)> = ak
            .keys
            .iter()
            .map(|k| (k.name.as_str(), k.value.as_str(), k.claims.clone()))
            .collect();
        lattice_auth::ApiKeyValidator::new(&ak.header_name, keys)
    });

    (
        validators,
        api_key_validator,
        auth.default.clone(),
        auth.resources.clone(),
        poll_handles,
    )
}

/// Initialized rate limiter and its default parameters.
type RateLimiterInit = (Option<Arc<dyn RateLimiter>>, Option<(u64, u64)>);

/// Initialize rate limiter from config.
fn init_rate_limiter(config: &Config) -> RateLimiterInit {
    let Some(rl) = config.rate_limit.as_ref().filter(|rl| rl.enabled) else {
        return (None, None);
    };

    match ValkeyRateLimiter::new(&rl.valkey_url, "lattice:rl:".into()) {
        Ok(limiter) => {
            let defaults = rl
                .default
                .as_ref()
                .and_then(|d| parse_window(&d.window).map(|w| (d.requests, w)));
            tracing::info!("rate limiter enabled");
            (Some(Arc::new(limiter)), defaults)
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to create rate limiter");
            (None, None)
        }
    }
}

/// Initialize entity cache from config.
fn init_cache(config: &Config) -> (Option<ValkeyCache>, u64) {
    let Some(entity) = config
        .cache
        .as_ref()
        .and_then(|c| c.entity.as_ref())
        .filter(|e| e.enabled)
    else {
        return (None, 300);
    };

    match ValkeyCache::new(&entity.valkey_url, entity.key_prefix.clone()) {
        Ok(cache) => {
            tracing::info!(ttl = entity.default_ttl, "entity cache enabled");
            (Some(cache), entity.default_ttl)
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to create entity cache");
            (None, entity.default_ttl)
        }
    }
}

/// Parse a duration string like `60s`, `5m`, `1h`, or plain seconds.
fn parse_window(window: &str) -> Option<u64> {
    let window = window.trim();

    window.strip_suffix('s').map_or_else(
        || {
            window.strip_suffix('m').map_or_else(
                || {
                    window
                        .strip_suffix('h')
                        .map_or_else(|| window.parse().ok(), |h| h.parse::<u64>().ok().map(|h| h * 3600))
                },
                |m| m.parse::<u64>().ok().map(|m| m * 60),
            )
        },
        |s| s.parse().ok(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_seconds() {
        assert_eq!(parse_window("60s"), Some(60));
        assert_eq!(parse_window("120s"), Some(120));
    }

    #[test]
    fn parse_window_minutes() {
        assert_eq!(parse_window("5m"), Some(300));
    }

    #[test]
    fn parse_window_hours() {
        assert_eq!(parse_window("1h"), Some(3600));
    }

    #[test]
    fn parse_window_plain_number() {
        assert_eq!(parse_window("60"), Some(60));
    }

    #[test]
    fn parse_window_invalid() {
        assert_eq!(parse_window("abc"), None);
    }
}
