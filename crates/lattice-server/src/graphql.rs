use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use lattice_execution::PlanExecutor;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct GraphqlRequest {
    pub query: String,
    #[serde(default, rename = "operationName")]
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

    let Some(planner) = &state.planner else {
        return Json(GraphqlResponse {
            data: None,
            errors: Some(vec![GraphqlError {
                message: "no supergraph loaded".into(),
            }]),
        });
    };

    // Plan the query.
    let plan = match planner.plan(&request.query, request.operation_name.as_deref()) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "query planning failed");
            return Json(GraphqlResponse {
                data: None,
                errors: Some(vec![GraphqlError {
                    message: format!("query planning failed: {e}"),
                }]),
            });
        }
    };

    // Execute the plan.
    let executor = PlanExecutor::new(&state.subgraph_client, &state.subgraph_urls);
    match executor.execute(&plan, request.variables.as_ref()).await {
        Ok(result) => {
            let errors: Option<Vec<GraphqlError>> = if result.errors.is_empty() {
                None
            } else {
                Some(
                    result
                        .errors
                        .into_iter()
                        .map(|e| GraphqlError { message: e.message })
                        .collect(),
                )
            };

            Json(GraphqlResponse {
                data: result.data,
                errors,
            })
        }
        Err(e) => {
            tracing::error!(error = %e, "query execution failed");
            Json(GraphqlResponse {
                data: None,
                errors: Some(vec![GraphqlError {
                    message: format!("execution failed: {e}"),
                }]),
            })
        }
    }
}
