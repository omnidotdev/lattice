use std::collections::HashMap;
use std::sync::Arc;

use lattice_auth::JwtValidator;
use lattice_cache::ValkeyCache;
use lattice_config::Config;
use lattice_execution::SubgraphClient;
use lattice_federation::FederationPlanner;
use lattice_plugin::PluginManager;
use lattice_rate_limit::{RateLimiter, ValkeyRateLimiter};

/// Shared application state for the axum server.
pub struct AppState {
    pub config: Config,
    pub planner: Option<FederationPlanner>,
    pub subgraph_client: SubgraphClient,
    /// Map of subgraph name to URL (from config).
    pub subgraph_urls: HashMap<String, String>,
    /// Optional JWT validator, present when auth is configured.
    pub jwt_validator: Option<JwtValidator>,
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

        let jwt_validator = config
            .auth
            .as_ref()
            .and_then(|a| a.jwt.as_ref())
            .map(JwtValidator::from_config);

        let (rate_limiter, rate_limit_defaults) = init_rate_limiter(&config);
        let (entity_cache, cache_ttl) = init_cache(&config);

        Self {
            config,
            planner,
            subgraph_client: SubgraphClient::new(),
            subgraph_urls,
            jwt_validator,
            rate_limiter,
            rate_limit_defaults,
            plugin_manager: None,
            entity_cache,
            cache_ttl,
        }
    }
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
