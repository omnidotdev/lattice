use std::hash::{Hash, Hasher};

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use lattice_cache::EntityCache;
use lattice_execution::{PlanExecutor, SubgraphRequest, SubgraphResponse};
use lattice_federation::FederationPlanner;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,
}

fn error_response(message: impl Into<String>) -> axum::response::Response {
    Json(GraphqlResponse {
        data: None,
        errors: Some(vec![GraphqlError {
            message: message.into(),
            extensions: None,
        }]),
    })
    .into_response()
}

/// Request body shape accepted off the wire. Unlike `GraphqlRequest`, `query` is
/// optional so an APQ hash-only request parses; the effective query is resolved
/// in the handler.
#[derive(Debug, Deserialize)]
pub struct RawGraphqlRequest {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default, rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
    #[serde(default)]
    pub extensions: Option<RequestExtensions>,
}

#[derive(Debug, Deserialize)]
pub struct RequestExtensions {
    #[serde(default, rename = "persistedQuery")]
    pub persisted_query: Option<PersistedQueryExtension>,
}

#[derive(Debug, Deserialize)]
pub struct PersistedQueryExtension {
    #[serde(default, rename = "sha256Hash")]
    pub sha256_hash: Option<String>,
}

/// Build an APQ protocol error carrying `message` and an `extensions.code`.
fn apq_error(message: &str, code: &str) -> axum::response::Response {
    Json(GraphqlResponse {
        data: None,
        errors: Some(vec![GraphqlError {
            message: message.to_string(),
            extensions: Some(serde_json::json!({ "code": code })),
        }]),
    })
    .into_response()
}

/// Resolve the effective query, applying Automatic Persisted Queries when a
/// `persistedQuery` extension is present. Returns `Err(response)` to short
/// circuit with an APQ protocol error or a missing-query error.
fn resolve_request_query(
    apq_store: Option<&crate::apq::ApqStore>,
    require_trusted: bool,
    raw_query: Option<String>,
    persisted: Option<&PersistedQueryExtension>,
) -> Result<String, Box<axum::response::Response>> {
    let hash = persisted.and_then(|p| p.sha256_hash.as_deref());

    // Trusted-documents mode: only operations in the preloaded manifest run, and
    // a freeform query (no hash, or a hash outside the trusted set) is rejected.
    if require_trusted {
        return match (hash, apq_store) {
            (Some(hash), Some(store)) => match crate::apq::resolve_trusted(hash, store) {
                crate::apq::ApqOutcome::Resolved(query) => Ok(query),
                _ => Err(Box::new(apq_error(
                    "PersistedQueryNotFound",
                    "PERSISTED_QUERY_NOT_FOUND",
                ))),
            },
            _ => Err(Box::new(apq_error(
                "operation is not a trusted document",
                "PERSISTED_QUERY_REQUIRED",
            ))),
        };
    }

    match (hash, apq_store) {
        // APQ request against a configured store.
        (Some(hash), Some(store)) => match crate::apq::resolve_apq(raw_query.as_deref(), hash, store) {
            crate::apq::ApqOutcome::Resolved(query) => Ok(query),
            crate::apq::ApqOutcome::NotFound => Err(Box::new(apq_error(
                "PersistedQueryNotFound",
                "PERSISTED_QUERY_NOT_FOUND",
            ))),
            crate::apq::ApqOutcome::HashMismatch => Err(Box::new(apq_error(
                "provided sha256Hash does not match the query",
                "PERSISTED_QUERY_INVALID",
            ))),
        },
        // A hash was sent but APQ is disabled: honor an inline query if present,
        // otherwise report that persisted queries are unsupported.
        (Some(_), None) => {
            raw_query.ok_or_else(|| Box::new(apq_error("PersistedQueryNotSupported", "PERSISTED_QUERY_NOT_SUPPORTED")))
        }
        // No APQ extension: a query body is required.
        (None, _) => raw_query.ok_or_else(|| Box::new(error_response("must provide a query string"))),
    }
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
/// Returns a rejection response (if any) and an optional contract key from the hook.
fn apply_on_request_hooks(
    state: &AppState,
    request: &mut GraphqlRequest,
) -> (Option<axum::response::Response>, Option<String>) {
    let Some(pm) = state.plugin_manager.as_ref() else {
        return (None, None);
    };

    let input = OnRequestInput {
        query: request.query.clone(),
        operation_name: request.operation_name.clone(),
        variables: request.variables.clone(),
        client_ip: None,
    };

    match pm.on_gateway_request(&input) {
        HookResult::Reject { message } => (Some(error_response(message)), None),
        HookResult::Modify { data } => {
            if let Some(q) = data.get("query").and_then(|v| v.as_str()) {
                q.clone_into(&mut request.query);
            }
            if let Some(v) = data.get("variables") {
                request.variables = Some(v.clone());
            }
            let contract = data.get("contract").and_then(|v| v.as_str()).map(String::from);
            (None, contract)
        }
        HookResult::Continue => (None, None),
    }
}

/// Resolve which planner to use based on contract selection.
///
/// Falls back to the default planner when no contract registry is configured.
fn resolve_planner<'a>(
    state: &'a AppState,
    hook_contract: Option<&str>,
    claim_contract: Option<&str>,
    header_contract: Option<&str>,
) -> Option<&'a FederationPlanner> {
    if let Some(registry) = &state.contract_registry {
        let contract_key = registry.resolve_key(hook_contract, claim_contract, header_contract);

        if let Some(variant) = registry.get(contract_key) {
            return Some(&variant.planner);
        }
    }

    // Fall back to the default planner.
    state.planner.as_ref()
}

/// Apply `on_gateway_response` plugin hooks, potentially modifying the response.
fn apply_on_response_hooks(
    state: &AppState,
    query: &str,
    data: Option<&serde_json::Value>,
    errors: Option<&Vec<GraphqlError>>,
) -> Option<axum::response::Response> {
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
        HookResult::Modify { data: modified } => Some(
            Json(GraphqlResponse {
                data: modified.get("data").cloned(),
                errors: errors.cloned(),
            })
            .into_response(),
        ),
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

/// Walk response data checking field access against the auth tree.
///
/// Returns a list of violation descriptions for Query-level fields.
fn check_auth_violations(
    tree: &lattice_auth::AuthTree,
    data: &serde_json::Value,
    is_authenticated: bool,
    user_scopes: &[String],
) -> Vec<String> {
    let Some(obj) = data.as_object() else {
        return vec![];
    };

    let mut violations = Vec::new();

    for field_name in obj.keys() {
        if let Some(req) = tree.requirement("Query", field_name) {
            if !lattice_auth::AuthTree::check_access(req, is_authenticated, user_scopes) {
                violations.push(format!("Query.{field_name}"));
            }
        }
    }

    violations
}

/// Handle incoming GraphQL POST requests.
#[allow(clippy::too_many_lines)]
pub async fn handle(
    State(shared): State<SharedState>,
    headers: axum::http::HeaderMap,
    claims: Option<axum::Extension<lattice_auth::Claims>>,
    Json(raw): Json<RawGraphqlRequest>,
) -> axum::response::Response {
    let start = std::time::Instant::now();
    let state = shared.load_full();

    // Resolve the effective query (applying APQ) before anything else.
    let persisted = raw.extensions.as_ref().and_then(|e| e.persisted_query.as_ref());
    let query = match resolve_request_query(state.apq_store.as_ref(), state.persisted_require, raw.query, persisted) {
        Ok(query) => query,
        Err(response) => return *response,
    };

    let mut request = GraphqlRequest {
        query,
        operation_name: raw.operation_name,
        variables: raw.variables,
    };

    tracing::info!(query = %request.query, "received graphql request");

    // Validate query limits before planning/execution.
    if let Some(limits) = &state.config.limits {
        if let Some(err) = crate::limits::validate_query(&request.query, limits) {
            tracing::warn!(error = %err, "query limit exceeded");
            lattice_telemetry::prometheus::record_http_request("POST", 400, "query", start.elapsed().as_secs_f64());
            return error_response(err);
        }
    }

    let (rejection, hook_contract) = apply_on_request_hooks(&state, &mut request);
    if let Some(rejection) = rejection {
        return rejection;
    }

    // on_graphql_parse hook (after gateway request, before planning).
    if let Some(pm) = state.plugin_manager.as_ref() {
        let parse_input = lattice_plugin::OnGraphqlParseInput {
            query: request.query.clone(),
            operation_name: request.operation_name.clone(),
        };
        match pm.on_graphql_parse(&parse_input) {
            lattice_plugin::HookResult::Reject { message } => {
                return error_response(message);
            }
            lattice_plugin::HookResult::Modify { data } => {
                if let Some(q) = data.get("query").and_then(|v| v.as_str()) {
                    q.clone_into(&mut request.query);
                }
            }
            lattice_plugin::HookResult::Continue => {}
        }
    }

    // Extract contract key from header (if contract registry is configured).
    let header_contract = state
        .contract_registry
        .as_ref()
        .and_then(|r| headers.get(r.header_name()).and_then(|v| v.to_str().ok()))
        .map(String::from);

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
                    lattice_telemetry::prometheus::record_cache_hit();
                    lattice_telemetry::prometheus::record_http_request(
                        "POST",
                        200,
                        "query",
                        start.elapsed().as_secs_f64(),
                    );
                    return Json(response).into_response();
                }
            }
            Ok(None) => {
                lattice_telemetry::prometheus::record_cache_miss();
            }
            Err(e) => tracing::warn!(error = %e, "cache lookup failed"),
        }
    }

    let Some(planner) = resolve_planner(
        &state,
        hook_contract.as_deref(),
        None, // Claims-based resolution can be added later.
        header_contract.as_deref(),
    ) else {
        return error_response("no supergraph loaded");
    };

    let plan = match planner.plan(&request.query, request.operation_name.as_deref()) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "query planning failed");
            return error_response(format!("query planning failed: {e}"));
        }
    };

    // on_graphql_validation and on_query_plan hooks (after planning, before execution).
    if let Some(pm) = state.plugin_manager.as_ref() {
        let validation_input = lattice_plugin::OnGraphqlValidationInput {
            query: request.query.clone(),
            operation_name: request.operation_name.clone(),
            is_valid: true,
            validation_errors: vec![],
        };
        if let lattice_plugin::HookResult::Reject { message } = pm.on_graphql_validation(&validation_input) {
            return error_response(message);
        }

        let plan_input = lattice_plugin::OnQueryPlanInput {
            query: request.query.clone(),
            operation_name: request.operation_name.clone(),
            plan_json: serde_json::to_value(&plan).unwrap_or_default(),
        };
        if let lattice_plugin::HookResult::Reject { message } = pm.on_query_plan(&plan_input) {
            return error_response(message);
        }
    }

    // Build executor with optional subgraph hooks.
    let executor = PlanExecutor::new(
        &state.subgraph_client,
        &state.subgraph_urls,
        &state.in_process_subgraphs,
    );

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

    let executor = match &state.dedup_layer {
        Some(dedup) => executor.with_dedup(dedup.clone()),
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
                        .map(|e| GraphqlError {
                            message: e.message,
                            extensions: None,
                        })
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

    // Enforce directive-based authorization.
    if let (Some(auth_tree), Some(data_val)) = (&state.auth_tree, &data) {
        let is_authenticated = claims.is_some();
        let user_scopes: Vec<String> = claims
            .as_ref()
            .and_then(|c| c.extra.get("scope"))
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        let violations = check_auth_violations(auth_tree, data_val, is_authenticated, &user_scopes);

        if !violations.is_empty() {
            let mode = state.authz_mode.as_deref().unwrap_or("filter");
            if mode == "reject" {
                let error_msgs: Vec<GraphqlError> = violations
                    .into_iter()
                    .map(|v| GraphqlError {
                        message: format!("unauthorized: {v}"),
                        extensions: None,
                    })
                    .collect();
                return Json(GraphqlResponse {
                    data: None,
                    errors: Some(error_msgs),
                })
                .into_response();
            }
            // In filter mode, null out unauthorized fields.
            if let Some(ref mut d) = data {
                if let Some(obj) = d.as_object_mut() {
                    for violation in &violations {
                        if let Some(field) = violation.strip_prefix("Query.") {
                            obj.insert(field.to_string(), serde_json::Value::Null);
                        }
                    }
                }
            }
        }
    }

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

    let op_type = if request.query.trim_start().starts_with("mutation") {
        "mutation"
    } else {
        "query"
    };
    lattice_telemetry::prometheus::record_http_request("POST", 200, op_type, start.elapsed().as_secs_f64());

    // Return multipart/mixed for @defer queries.
    if lattice_execution::query_has_defer(&request.query) {
        let body = lattice_execution::encode_multipart_patches(
            response.data.as_ref().unwrap_or(&serde_json::Value::Null),
            &[], // No separate patches yet — all data is merged eagerly.
        );

        return axum::http::Response::builder()
            .status(200)
            .header("content-type", "multipart/mixed; boundary=---")
            .body(axum::body::Body::from(body))
            .unwrap()
            .into_response();
    }

    Json(response).into_response()
}

#[cfg(test)]
mod persisted_tests {
    use super::*;
    use crate::apq::{ApqStore, sha256_hex};

    fn ext(hash: &str) -> PersistedQueryExtension {
        PersistedQueryExtension {
            sha256_hash: Some(hash.to_string()),
        }
    }

    #[test]
    fn trusted_runs_a_known_operation_by_hash() {
        let store = ApqStore::new(10);
        let query = "{ me { id } }";
        let hash = sha256_hex(query);
        store.insert(hash.clone(), query.to_string());

        let out = resolve_request_query(Some(&store), true, None, Some(&ext(&hash)));
        assert_eq!(out.unwrap(), query);
    }

    #[test]
    fn trusted_rejects_an_unknown_hash() {
        let store = ApqStore::new(10);
        let out = resolve_request_query(Some(&store), true, None, Some(&ext("deadbeef")));
        assert!(out.is_err());
    }

    #[test]
    fn trusted_rejects_a_freeform_query() {
        let store = ApqStore::new(10);
        let out = resolve_request_query(Some(&store), true, Some("{ me { id } }".to_string()), None);
        assert!(out.is_err());
    }

    #[test]
    fn trusted_refuses_an_untrusted_hash_even_with_a_matching_body() {
        let store = ApqStore::new(10);
        let query = "{ evil { secrets } }";
        let hash = sha256_hex(query);
        let out = resolve_request_query(Some(&store), true, Some(query.to_string()), Some(&ext(&hash)));
        assert!(out.is_err());
        assert_eq!(store.get(&hash), None, "untrusted operation must not be registered");
    }

    #[test]
    fn apq_mode_still_registers_and_resolves() {
        let store = ApqStore::new(10);
        let query = "{ me { id } }";
        let hash = sha256_hex(query);
        let out = resolve_request_query(Some(&store), false, Some(query.to_string()), Some(&ext(&hash)));
        assert_eq!(out.unwrap(), query);
        assert_eq!(store.get(&hash), Some(query.to_string()));
    }

    #[test]
    fn freeform_query_runs_without_persisted_queries() {
        let out = resolve_request_query(None, false, Some("{ me { id } }".to_string()), None);
        assert_eq!(out.unwrap(), "{ me { id } }");
    }
}
