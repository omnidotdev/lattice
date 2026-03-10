use thiserror::Error;

#[derive(Debug, Error)]
pub enum FederationError {
    #[error("failed to parse supergraph SDL: {0}")]
    Parse(String),
    #[error("failed to build query planner: {0}")]
    Planner(String),
    #[error("query parse error: {0}")]
    QueryParse(String),
    #[error("normalization error: {0}")]
    Normalization(String),
    #[error("query plan error: {0}")]
    Plan(String),
}
