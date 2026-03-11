use lattice_execution::PlanExecutor;

use super::JsonRpcResponse;
use crate::SharedState;

/// Return the list of available MCP tools.
pub fn list() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "sdl",
            "description": "Return the raw supergraph SDL schema",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        serde_json::json!({
            "name": "search",
            "description": "Search the GraphQL schema for types, fields, or directives matching a query string",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query (case-insensitive)" },
                },
                "required": ["query"],
            },
        }),
        serde_json::json!({
            "name": "introspect",
            "description": "Run a GraphQL introspection query and return the full schema JSON",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        serde_json::json!({
            "name": "execute",
            "description": "Execute a GraphQL query against the federated gateway",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "GraphQL query string" },
                    "variables": { "type": "object", "description": "Query variables" },
                    "operationName": { "type": "string", "description": "Operation name" },
                },
                "required": ["query"],
            },
        }),
    ]
}

/// Dispatch a `tools/call` request to the appropriate tool.
pub async fn call(
    shared: SharedState,
    id: Option<serde_json::Value>,
    params: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let params = params.unwrap_or_else(|| serde_json::json!({}));
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let state = shared.load_full();
    let (active_sdl, active_planner) = resolve_mcp_context(&state);

    match tool_name {
        "sdl" => {
            let sdl = active_sdl.unwrap_or("No supergraph SDL loaded");
            content_response(id, sdl)
        }
        "search" => {
            let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let sdl = active_sdl.unwrap_or("");
            let results = search_sdl(sdl, query);
            if results.is_empty() {
                content_response(id, "No matches found")
            } else {
                content_response(id, &results.join("\n"))
            }
        }
        "introspect" => execute_query_with_planner(active_planner, &state, id, INTROSPECTION_QUERY, None, None).await,
        "execute" => {
            let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let variables = arguments.get("variables");
            let op_name = arguments.get("operationName").and_then(|v| v.as_str());
            execute_query_with_planner(active_planner, &state, id, query, variables, op_name).await
        }
        _ => JsonRpcResponse::error(id, -32602, format!("unknown tool: {tool_name}")),
    }
}

/// Build a text content response for MCP tool results.
fn content_response(id: Option<serde_json::Value>, text: &str) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content": [{ "type": "text", "text": text }],
        }),
    )
}

/// Resolve the active SDL and planner for MCP, respecting contracts.
fn resolve_mcp_context(state: &crate::AppState) -> (Option<&str>, Option<&lattice_federation::FederationPlanner>) {
    if let Some(registry) = &state.contract_registry {
        let key = registry.default_key();
        if let Some(variant) = registry.get(key) {
            return (Some(&variant.sdl), Some(&variant.planner));
        }
    }
    (state.supergraph_sdl.as_deref(), state.planner.as_ref())
}

/// Search SDL for lines matching the query (case-insensitive).
fn search_sdl(sdl: &str, query: &str) -> Vec<String> {
    if query.is_empty() {
        return vec![];
    }

    let query_lower = query.to_lowercase();

    sdl.lines()
        .filter(|line| line.to_lowercase().contains(&query_lower))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Plan and execute a GraphQL query, returning the result as an MCP tool response.
async fn execute_query_with_planner(
    planner: Option<&lattice_federation::FederationPlanner>,
    state: &crate::AppState,
    id: Option<serde_json::Value>,
    query: &str,
    variables: Option<&serde_json::Value>,
    operation_name: Option<&str>,
) -> JsonRpcResponse {
    let Some(planner) = planner else {
        return JsonRpcResponse::error(id, -32603, "no supergraph loaded");
    };

    let plan = match planner.plan(query, operation_name) {
        Ok(plan) => plan,
        Err(e) => {
            return JsonRpcResponse::error(id, -32603, format!("query planning failed: {e}"));
        }
    };

    let executor = PlanExecutor::new(&state.subgraph_client, &state.subgraph_urls);

    match executor.execute(&plan, variables).await {
        Ok(result) => {
            let errors: Vec<serde_json::Value> = result
                .errors
                .iter()
                .map(|e| serde_json::json!({"message": &e.message}))
                .collect();

            let response = if errors.is_empty() {
                serde_json::json!({ "data": result.data })
            } else {
                serde_json::json!({ "data": result.data, "errors": errors })
            };

            let text = serde_json::to_string_pretty(&response).unwrap_or_default();
            content_response(id, &text)
        }
        Err(e) => JsonRpcResponse::error(id, -32603, format!("execution failed: {e}")),
    }
}

const INTROSPECTION_QUERY: &str = r"query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      ...FullType
    }
    directives {
      name
      description
      locations
      args { ...InputValue }
    }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
    deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name
    description
    isDeprecated
    deprecationReason
  }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
            }
          }
        }
      }
    }
  }
}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_matching_lines() {
        let sdl = "type Query {\n  me: User\n}\n\ntype User {\n  id: ID!\n  name: String\n}";
        let results = search_sdl(sdl, "User");
        assert_eq!(results.len(), 2);
        assert!(results[0].contains("User"));
    }

    #[test]
    fn search_case_insensitive() {
        let sdl = "type Query {\n  me: User\n}";
        let results = search_sdl(sdl, "query");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_empty_query_returns_nothing() {
        let sdl = "type Query { me: User }";
        let results = search_sdl(sdl, "");
        assert!(results.is_empty());
    }
}
