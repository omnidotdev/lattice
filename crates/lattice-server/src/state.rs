use std::collections::HashMap;
use std::sync::Arc;

use lattice_auth::JwtValidator;
use lattice_config::Config;
use lattice_execution::SubgraphClient;
use lattice_federation::FederationPlanner;
use lattice_rate_limit::RateLimiter;

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

        Self {
            config,
            planner,
            subgraph_client: SubgraphClient::new(),
            subgraph_urls,
            jwt_validator,
            rate_limiter: None,
            rate_limit_defaults: None,
        }
    }
}
