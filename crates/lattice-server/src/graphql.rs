use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use lattice_execution::PlanExecutor;
use lattice_plugin::{HookResult, OnRequestInput, OnResponseInput};

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

#[derive(Debug, Clone, Serialize)]
pub struct GraphqlError {
    pub message: String,
}

fn error_response(message: impl Into<String>) -> Json<GraphqlResponse> {
    Json(GraphqlResponse {
        data: None,
        errors: Some(vec![GraphqlError {
            message: message.into(),
        }]),
    })
}

/// Apply `on_request` plugin hooks, potentially modifying the request.
///
/// Returns `Some(response)` if a plugin rejected the request, `None` to continue.
fn apply_on_request_hooks(state: &AppState, request: &mut GraphqlRequest) -> Option<Json<GraphqlResponse>> {
    let pm = state.plugin_manager.as_ref()?;

    let input = OnRequestInput {
        query: request.query.clone(),
        operation_name: request.operation_name.clone(),
        variables: request.variables.clone(),
        client_ip: None,
    };

    match pm.on_request(&input) {
        HookResult::Reject { message } => Some(error_response(message)),
        HookResult::Modify { data } => {
            if let Some(q) = data.get("query").and_then(|v| v.as_str()) {
                q.clone_into(&mut request.query);
            }
            if let Some(v) = data.get("variables") {
                request.variables = Some(v.clone());
            }
            None
        }
        HookResult::Continue => None,
    }
}

/// Apply `on_response` plugin hooks, potentially modifying the response.
fn apply_on_response_hooks(
    state: &AppState,
    query: &str,
    data: Option<&serde_json::Value>,
    errors: Option<&Vec<GraphqlError>>,
) -> Option<Json<GraphqlResponse>> {
    let pm = state.plugin_manager.as_ref()?;

    let input = OnResponseInput {
        query: query.to_owned(),
        data: data.cloned(),
        errors: errors
            .map(|errs| {
                errs.iter()
                    .map(|e| serde_json::json!({"message": &e.message}))
                    .collect()
            })
            .unwrap_or_default(),
    };

    match pm.on_response(&input) {
        HookResult::Modify { data: modified } => Some(Json(GraphqlResponse {
            data: modified.get("data").cloned(),
            errors: errors.cloned(),
        })),
        HookResult::Continue | HookResult::Reject { .. } => None,
    }
}

/// Handle incoming GraphQL POST requests.
pub async fn handle(
    State(state): State<Arc<AppState>>,
    Json(mut request): Json<GraphqlRequest>,
) -> Json<GraphqlResponse> {
    tracing::info!(query = %request.query, "received graphql request");

    if let Some(rejection) = apply_on_request_hooks(&state, &mut request) {
        return rejection;
    }

    let Some(planner) = &state.planner else {
        return error_response("no supergraph loaded");
    };

    let plan = match planner.plan(&request.query, request.operation_name.as_deref()) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "query planning failed");
            return error_response(format!("query planning failed: {e}"));
        }
    };

    let executor = PlanExecutor::new(&state.subgraph_client, &state.subgraph_urls);
    let (data, errors) = match executor.execute(&plan, request.variables.as_ref()).await {
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
            (result.data, errors)
        }
        Err(e) => {
            tracing::error!(error = %e, "query execution failed");
            return error_response(format!("execution failed: {e}"));
        }
    };

    if let Some(modified) = apply_on_response_hooks(&state, &request.query, data.as_ref(), errors.as_ref()) {
        return modified;
    }

    Json(GraphqlResponse { data, errors })
}
