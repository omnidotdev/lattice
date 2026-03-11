mod tools;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use crate::SharedState;

/// Build the MCP router.
pub fn router() -> Router<SharedState> {
    Router::new().route("/", post(handle_request))
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<serde_json::Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Handle MCP JSON-RPC requests over HTTP POST.
async fn handle_request(State(shared): State<SharedState>, Json(request): Json<JsonRpcRequest>) -> Response {
    // Validate JSON-RPC version.
    if request.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::error(request.id, -32600, "invalid JSON-RPC version")).into_response();
    }

    // Notifications (no id) return 202 Accepted per MCP Streamable HTTP spec.
    if request.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            request.id,
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "lattice-gateway",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ),
        "tools/list" => JsonRpcResponse::success(request.id, serde_json::json!({ "tools": tools::list() })),
        "tools/call" => tools::call(shared, request.id, request.params).await,
        _ => JsonRpcResponse::error(request.id, -32601, "method not found"),
    };

    Json(response).into_response()
}
