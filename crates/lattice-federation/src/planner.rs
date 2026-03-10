use graphql_tools::parser::parse_query;
use hive_router_query_planner::{
    ast::normalization::normalize_operation,
    graph::PlannerOverrideContext,
    planner::{Planner, plan_nodes::QueryPlan},
    utils::cancellation::CancellationToken,
};

use crate::FederationError;

/// Thin wrapper around `hive-router-query-planner`.
pub struct FederationPlanner {
    inner: Planner,
}

impl FederationPlanner {
    /// Create a planner from a supergraph SDL string.
    ///
    /// # Errors
    ///
    /// Returns `FederationError::Parse` if the SDL is not valid GraphQL,
    /// or `FederationError::Planner` if the planner cannot be built from the parsed schema.
    pub fn from_sdl(sdl: &str) -> Result<Self, FederationError> {
        let parsed = graphql_tools::parser::parse_schema(sdl)
            .map_err(|e| FederationError::Parse(format!("{e:?}")))?
            .into_static();

        let inner = Planner::new_from_supergraph(&parsed).map_err(|e| FederationError::Planner(format!("{e:?}")))?;

        Ok(Self { inner })
    }

    /// Plan a client query against the supergraph.
    ///
    /// Parses the query string, normalizes it against the supergraph schema,
    /// and produces a query plan describing which subgraphs to fetch from.
    ///
    /// # Errors
    ///
    /// Returns `FederationError` if the query cannot be parsed, normalized, or planned.
    pub fn plan(&self, query: &str, operation_name: Option<&str>) -> Result<QueryPlan, FederationError> {
        let parsed = parse_query::<String>(query)
            .map_err(|e| FederationError::QueryParse(format!("{e:?}")))?
            .into_static();

        let normalized = normalize_operation(&self.inner.supergraph, &parsed, operation_name)
            .map_err(|e| FederationError::Normalization(format!("{e:?}")))?;

        let override_ctx = PlannerOverrideContext::from_percentage(0.0);
        let cancel_token = CancellationToken::new();

        self.inner
            .plan_from_normalized_operation(normalized.executable_operation(), override_ctx, &cancel_token)
            .map_err(|e| FederationError::Plan(format!("{e:?}")))
    }

    /// Access the inner planner for query planning.
    #[must_use]
    pub const fn inner(&self) -> &Planner {
        &self.inner
    }
}
