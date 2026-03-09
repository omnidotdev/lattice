use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GraphqlRequest {
    pub query: String,
    #[serde(default)]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct GraphqlResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Serialize)]
pub struct GraphqlError {
    pub message: String,
}

/// Handle incoming GraphQL POST requests.
pub async fn handle(State(_state): State<Arc<AppState>>, Json(request): Json<GraphqlRequest>) -> Json<GraphqlResponse> {
    // TODO: wire to federation planner + executor
    tracing::info!(query = %request.query, "received graphql request");

    Json(GraphqlResponse {
        data: Some(serde_json::json!({ "__typename": "Query" })),
        errors: None,
    })
}
