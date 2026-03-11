use std::hash::{Hash, Hasher};

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use lattice_cache::EntityCache;
use lattice_execution::{PlanExecutor, SubgraphRequest, SubgraphResponse};
use lattice_plugin::{
    HookResult, OnHttpResponseInput, OnOperationResponseInput, OnRequestInput, OnResponseInput, OnSubgraphRequestInput,
    OnSubgraphResponseInput,
};

use crate::{AppState, SharedState};

#[derive(Debug, Deserialize)]
pub struct GraphqlRequest {
    pub query: String,
    #[serde(default, rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GraphqlResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// Build a cache key from query, operation name, and variables.
fn cache_key(query: &str, operation_name: Option<&str>, variables: Option<&serde_json::Value>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    query.hash(&mut hasher);
    operation_name.hash(&mut hasher);
    if let Some(vars) = variables {
        vars.to_string().hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Apply `on_gateway_request` plugin hooks, potentially modifying the request.
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

    match pm.on_gateway_request(&input) {
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

/// Apply `on_gateway_response` plugin hooks, potentially modifying the response.
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

    match pm.on_gateway_response(&input) {
        HookResult::Modify { data: modified } => Some(Json(GraphqlResponse {
            data: modified.get("data").cloned(),
            errors: errors.cloned(),
        })),
        HookResult::Continue | HookResult::Reject { .. } => None,
    }
}

/// Apply `on_operation_response` plugin hook after execution.
fn apply_on_operation_response_hooks(
    state: &AppState,
    query: &str,
    operation_name: Option<&str>,
    data: &mut Option<serde_json::Value>,
    errors: Option<&Vec<GraphqlError>>,
) {
    let Some(pm) = state.plugin_manager.as_ref() else {
        return;
    };

    let input = OnOperationResponseInput {
        query: query.to_owned(),
        operation_name: operation_name.map(String::from),
        data: data.clone(),
        errors: errors
            .as_ref()
            .map(|errs| {
                errs.iter()
                    .map(|e| serde_json::json!({"message": &e.message}))
                    .collect()
            })
            .unwrap_or_default(),
    };

    if let HookResult::Modify { data: modified } = pm.on_operation_response(&input) {
        if let Some(d) = modified.get("data") {
            *data = Some(d.clone());
        }
    }
}

/// Apply `on_http_response` plugin hook before sending.
fn apply_on_http_response_hooks(state: &AppState, response: &mut GraphqlResponse) {
    let Some(pm) = state.plugin_manager.as_ref() else {
        return;
    };

    let input = OnHttpResponseInput {
        status_code: 200,
        body: serde_json::to_value(&*response).ok(),
    };

    if let HookResult::Modify { data: modified } = pm.on_http_response(&input) {
        if let Ok(modified_response) = serde_json::from_value::<GraphqlResponse>(modified) {
            *response = modified_response;
        }
    }
}

/// Build subgraph request hook closure for plugin dispatch.
fn make_subgraph_request_hook(
    pm: &lattice_plugin::PluginManager,
) -> impl Fn(&str, &mut SubgraphRequest) -> Result<(), String> + '_ {
    move |name: &str, req: &mut SubgraphRequest| {
        let input = OnSubgraphRequestInput {
            subgraph_name: name.to_owned(),
            url: req.url.clone(),
            query: req.query.clone(),
            variables: req.variables.clone(),
            headers: req.headers.clone(),
        };

        match pm.on_subgraph_request(&input) {
            HookResult::Reject { message } => Err(message),
            HookResult::Modify { data } => {
                if let Some(q) = data.get("query").and_then(|v| v.as_str()) {
                    q.clone_into(&mut req.query);
                }
                if let Some(v) = data.get("variables") {
                    req.variables = Some(v.clone());
                }
                Ok(())
            }
            HookResult::Continue => Ok(()),
        }
    }
}

/// Build subgraph response hook closure for plugin dispatch.
fn make_subgraph_response_hook(pm: &lattice_plugin::PluginManager) -> impl Fn(&str, &mut SubgraphResponse) + '_ {
    move |name: &str, resp: &mut SubgraphResponse| {
        let input = OnSubgraphResponseInput {
            subgraph_name: name.to_owned(),
            data: resp.data.clone(),
            errors: resp
                .errors
                .as_ref()
                .map(|errs| {
                    errs.iter()
                        .map(|e| serde_json::json!({"message": &e.message}))
                        .collect()
                })
                .unwrap_or_default(),
        };

        if let HookResult::Modify { data: modified } = pm.on_subgraph_response(&input) {
            if let Some(d) = modified.get("data") {
                resp.data = Some(d.clone());
            }
        }
    }
}

/// Handle incoming GraphQL POST requests.
pub async fn handle(
    State(shared): State<SharedState>,
    Json(mut request): Json<GraphqlRequest>,
) -> Json<GraphqlResponse> {
    let state = shared.load_full();
    tracing::info!(query = %request.query, "received graphql request");

    if let Some(rejection) = apply_on_request_hooks(&state, &mut request) {
        return rejection;
    }

    // Check response cache.
    let key = cache_key(
        &request.query,
        request.operation_name.as_deref(),
        request.variables.as_ref(),
    );

    if let Some(cache) = &state.entity_cache {
        match cache.get(&key).await {
            Ok(Some(cached)) => {
                if let Ok(response) = serde_json::from_str::<GraphqlResponse>(&cached) {
                    tracing::debug!("cache hit");
                    return Json(response);
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "cache lookup failed"),
        }
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

    // Build executor with optional subgraph hooks.
    let executor = PlanExecutor::new(&state.subgraph_client, &state.subgraph_urls);

    let req_hook;
    let resp_hook;
    let executor = match &state.plugin_manager {
        Some(pm) => {
            req_hook = make_subgraph_request_hook(pm);
            resp_hook = make_subgraph_response_hook(pm);
            executor
                .with_subgraph_request_hook(&req_hook)
                .with_subgraph_response_hook(&resp_hook)
        }
        None => executor,
    };

    let (mut data, errors) = match executor.execute(&plan, request.variables.as_ref()).await {
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

    // Apply on_operation_response hook (after execution, before caching).
    apply_on_operation_response_hooks(
        &state,
        &request.query,
        request.operation_name.as_deref(),
        &mut data,
        errors.as_ref(),
    );

    if let Some(modified) = apply_on_response_hooks(&state, &request.query, data.as_ref(), errors.as_ref()) {
        return modified;
    }

    let mut response = GraphqlResponse { data, errors };

    // Cache successful responses.
    if response.errors.is_none() {
        if let Some(cache) = &state.entity_cache {
            if let Ok(serialized) = serde_json::to_string(&response) {
                if let Err(e) = cache.set(&key, &serialized, state.cache_ttl).await {
                    tracing::warn!(error = %e, "failed to cache response");
                }
            }
        }
    }

    // Apply on_http_response hook (final hook before sending).
    apply_on_http_response_hooks(&state, &mut response);

    Json(response)
}
