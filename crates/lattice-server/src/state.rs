use std::collections::HashMap;

use lattice_config::Config;
use lattice_execution::SubgraphClient;
use lattice_federation::FederationPlanner;

/// Shared application state for the axum server.
pub struct AppState {
    pub config: Config,
    pub planner: Option<FederationPlanner>,
    pub subgraph_client: SubgraphClient,
    /// Map of subgraph name to URL (from config).
    pub subgraph_urls: HashMap<String, String>,
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

        Self {
            config,
            planner,
            subgraph_client: SubgraphClient::new(),
            subgraph_urls,
        }
    }
}
