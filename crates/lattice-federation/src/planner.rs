use hive_router_query_planner::planner::Planner;

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

        let inner = Planner::new_from_supergraph(&parsed).map_err(|e| FederationError::Planner(format!("{e}")))?;

        Ok(Self { inner })
    }

    /// Access the inner planner for query planning.
    #[must_use]
    pub const fn inner(&self) -> &Planner {
        &self.inner
    }
}
