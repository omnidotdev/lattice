use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::AppState;

// TODO: wire operation_name and variables to query planner
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
pub async fn handle(State(state): State<Arc<AppState>>, Json(request): Json<GraphqlRequest>) -> Json<GraphqlResponse> {
    tracing::info!(query = %request.query, "received graphql request");

    if state.planner.is_none() {
        return Json(GraphqlResponse {
            data: None,
            errors: Some(vec![GraphqlError {
                message: "no supergraph loaded".into(),
            }]),
        });
    }

    // TODO: integrate query planning + execution once hive crate API is validated
    Json(GraphqlResponse {
        data: Some(serde_json::json!({ "__typename": "Query" })),
        errors: None,
    })
}
