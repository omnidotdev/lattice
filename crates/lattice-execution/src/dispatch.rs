use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("subgraph request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("failed to parse subgraph response: {0}")]
    Parse(String),
    #[error("subgraph not found: {0}")]
    SubgraphNotFound(String),
}

pub struct SubgraphRequest {
    pub url: String,
    pub query: String,
    pub operation_name: Option<String>,
    pub variables: Option<serde_json::Value>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
pub struct SubgraphResponse {
    pub data: Option<serde_json::Value>,
    pub errors: Option<Vec<SubgraphError>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SubgraphError {
    pub message: String,
}

#[derive(Serialize)]
struct SubgraphRequestBody {
    query: String,
    #[serde(rename = "operationName", skip_serializing_if = "Option::is_none")]
    operation_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<serde_json::Value>,
}

/// Trait for in-process subgraph execution.
/// Implement this to execute GraphQL queries without an HTTP hop.
#[async_trait]
pub trait SubgraphExecutor: Send + Sync {
    /// Execute a GraphQL query and return the response.
    async fn execute_graphql(&self, request: SubgraphRequest) -> Result<SubgraphResponse, ExecutionError>;
}

/// HTTP client for dispatching GraphQL requests to subgraphs.
pub struct SubgraphClient {
    http: reqwest::Client,
}

impl SubgraphClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    /// Execute a GraphQL request against a subgraph.
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError::Request` if the HTTP request fails or the
    /// response cannot be deserialized.
    pub async fn execute(&self, request: SubgraphRequest) -> Result<SubgraphResponse, ExecutionError> {
        let body = SubgraphRequestBody {
            query: request.query,
            operation_name: request.operation_name,
            variables: request.variables,
        };

        let mut builder = self.http.post(&request.url).json(&body);

        for (key, value) in &request.headers {
            builder = builder.header(key, value);
        }

        let response = builder.send().await?.json::<SubgraphResponse>().await?;

        Ok(response)
    }
}

#[async_trait]
impl SubgraphExecutor for SubgraphClient {
    async fn execute_graphql(&self, request: SubgraphRequest) -> Result<SubgraphResponse, ExecutionError> {
        self.execute(request).await
    }
}

impl Default for SubgraphClient {
    fn default() -> Self {
        Self::new()
    }
}
