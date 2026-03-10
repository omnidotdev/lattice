use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::protocol::{ServerMessage, SubscribePayload};

type UpstreamStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect to an upstream subgraph via WebSocket and forward subscription
/// messages back through the `tx` channel.
///
/// # Errors
///
/// Returns an error if the upstream connection fails.
pub async fn forward_subscription(
    id: &str,
    payload: &SubscribePayload,
    ws_url: &str,
    tx: &mpsc::Sender<ServerMessage>,
) -> Result<(), String> {
    let mut upstream = connect_and_subscribe(id, payload, ws_url).await?;

    // Forward messages from upstream to client.
    while let Some(msg_result) = upstream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "upstream websocket error");
                break;
            }
        };

        let tungstenite::Message::Text(text) = msg else {
            continue;
        };

        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        let msg_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let msg_id = parsed.get("id").and_then(|i| i.as_str()).unwrap_or(id);

        match msg_type {
            "next" => {
                if let Some(payload) = parsed.get("payload").cloned() {
                    let _ = tx
                        .send(ServerMessage::Next {
                            id: msg_id.to_owned(),
                            payload,
                        })
                        .await;
                }
            }
            "error" => {
                let errors = parsed
                    .get("payload")
                    .cloned()
                    .map(|p| if p.is_array() { p } else { serde_json::json!([p]) })
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();

                let _ = tx
                    .send(ServerMessage::Error {
                        id: msg_id.to_owned(),
                        payload: errors,
                    })
                    .await;
                break;
            }
            "complete" => {
                let _ = tx.send(ServerMessage::Complete { id: msg_id.to_owned() }).await;
                break;
            }
            "ping" => {
                // Respond to upstream ping.
                let pong = serde_json::json!({"type": "pong"});
                let _ = upstream.send(tungstenite::Message::Text(pong.to_string().into())).await;
            }
            _ => {
                tracing::debug!(msg_type = %msg_type, "ignoring unknown upstream message type");
            }
        }
    }

    // Close upstream connection.
    let _ = upstream.close(None).await;

    Ok(())
}

/// Connect to an upstream subgraph, perform the `ConnectionInit`/`ConnectionAck`
/// handshake, and send the `Subscribe` message.
async fn connect_and_subscribe(id: &str, payload: &SubscribePayload, ws_url: &str) -> Result<UpstreamStream, String> {
    let request = tungstenite::http::Request::builder()
        .uri(ws_url)
        .header("Sec-WebSocket-Protocol", "graphql-transport-ws")
        .header("Host", host_from_url(ws_url))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .body(())
        .map_err(|e| format!("failed to build request: {e}"))?;

    let (mut upstream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("failed to connect to upstream: {e}"))?;

    // Send `ConnectionInit`.
    let init_msg = serde_json::json!({"type": "connection_init"});
    upstream
        .send(tungstenite::Message::Text(init_msg.to_string().into()))
        .await
        .map_err(|e| format!("failed to send init: {e}"))?;

    // Wait for `ConnectionAck`.
    let ack = upstream
        .next()
        .await
        .ok_or("upstream closed before ack")?
        .map_err(|e| format!("upstream error: {e}"))?;

    if let tungstenite::Message::Text(text) = &ack {
        let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("invalid ack: {e}"))?;
        if parsed.get("type").and_then(|t| t.as_str()) != Some("connection_ack") {
            return Err(format!("expected connection_ack, got: {parsed}"));
        }
    } else {
        return Err("expected text message for connection_ack".into());
    }

    // Send `Subscribe`.
    let subscribe_msg = serde_json::json!({
        "type": "subscribe",
        "id": id,
        "payload": {
            "query": payload.query,
            "operationName": payload.operation_name,
            "variables": payload.variables,
        }
    });
    upstream
        .send(tungstenite::Message::Text(subscribe_msg.to_string().into()))
        .await
        .map_err(|e| format!("failed to send subscribe: {e}"))?;

    Ok(upstream)
}

/// Extract host from a WebSocket URL for the Host header.
fn host_from_url(url: &str) -> String {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host_from_ws_url() {
        assert_eq!(host_from_url("ws://localhost:3000/graphql"), "localhost:3000");
        assert_eq!(host_from_url("wss://api.example.com/graphql"), "api.example.com");
    }
}
