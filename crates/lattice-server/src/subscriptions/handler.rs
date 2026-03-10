use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::AppState;

use super::protocol::{ClientMessage, ServerMessage, SubscribePayload, close_code};
use super::upstream;

/// Maximum time (ms) to wait for `ConnectionInit` after WebSocket upgrade.
const INIT_TIMEOUT_MS: u64 = 5_000;

/// Handle an upgraded WebSocket connection using the graphql-transport-ws protocol.
pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_sink, mut ws_stream) = socket.split();

    if !initialize_connection(&mut ws_sink, &mut ws_stream).await {
        return;
    }

    // Connection is initialized. Process messages.
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(64);
    let mut active_subscriptions: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

    loop {
        tokio::select! {
            // Incoming client message.
            msg = ws_stream.next() => {
                let Some(Ok(msg)) = msg else { break; };

                match parse_client_message(&msg) {
                    Some(ClientMessage::Subscribe { id, payload }) => {
                        if active_subscriptions.contains_key(&id) {
                            let _ = ws_sink
                                .send(Message::Close(Some(CloseFrame {
                                    code: close_code::SUBSCRIBER_ALREADY_EXISTS,
                                    reason: format!("subscriber for {id} already exists").into(),
                                })))
                                .await;
                            break;
                        }

                        let handle = spawn_subscription(
                            id.clone(),
                            payload,
                            state.clone(),
                            tx.clone(),
                        );
                        active_subscriptions.insert(id, handle);
                    }
                    Some(ClientMessage::Complete { id }) => {
                        if let Some(handle) = active_subscriptions.remove(&id) {
                            handle.abort();
                        }
                        // Acknowledge completion.
                        let _ = send_message(&mut ws_sink, &ServerMessage::Complete { id }).await;
                    }
                    Some(ClientMessage::Ping { payload }) => {
                        let _ = send_message(&mut ws_sink, &ServerMessage::Pong { payload }).await;
                    }
                    Some(ClientMessage::Pong { .. }) => {
                        // Client responded to our ping, nothing to do.
                    }
                    Some(ClientMessage::ConnectionInit { .. }) => {
                        let _ = ws_sink
                            .send(Message::Close(Some(CloseFrame {
                                code: close_code::TOO_MANY_INIT_REQUESTS,
                                reason: "too many initialisation requests".into(),
                            })))
                            .await;
                        break;
                    }
                    None => {
                        // Unparseable message — close per protocol.
                        if matches!(msg, Message::Text(_)) {
                            let _ = ws_sink
                                .send(Message::Close(Some(CloseFrame {
                                    code: close_code::INVALID_MESSAGE,
                                    reason: "invalid message".into(),
                                })))
                                .await;
                            break;
                        }
                        // Ignore binary/ping/pong/close frames.
                    }
                }
            }
            // Outgoing server message from subscription tasks.
            Some(server_msg) = rx.recv() => {
                if send_message(&mut ws_sink, &server_msg).await.is_err() {
                    break;
                }

                // If this was Complete or Error, remove the subscription.
                match &server_msg {
                    ServerMessage::Complete { id } | ServerMessage::Error { id, .. } => {
                        if let Some(handle) = active_subscriptions.remove(id) {
                            handle.abort();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Clean up all active subscriptions.
    for (_, handle) in active_subscriptions {
        handle.abort();
    }
}

/// Perform the `ConnectionInit`/`ConnectionAck` handshake.
///
/// Returns `true` if the connection was successfully initialized.
async fn initialize_connection(
    ws_sink: &mut (impl SinkExt<Message> + Unpin),
    ws_stream: &mut (impl StreamExt<Item = Result<Message, axum::Error>> + Unpin),
) -> bool {
    let init_result = tokio::time::timeout(
        std::time::Duration::from_millis(INIT_TIMEOUT_MS),
        wait_for_init(ws_stream),
    )
    .await;

    match init_result {
        Ok(Some(true)) => {
            let ack = ServerMessage::ConnectionAck { payload: None };
            send_message(ws_sink, &ack).await.is_ok()
        }
        Ok(Some(false) | None) => {
            let _ = ws_sink
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::UNAUTHORIZED,
                    reason: "connection not initialized".into(),
                })))
                .await;
            false
        }
        Err(_) => {
            let _ = ws_sink
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::INIT_TIMEOUT,
                    reason: "connection initialisation timeout".into(),
                })))
                .await;
            false
        }
    }
}

/// Wait for a `ConnectionInit` message. Returns `Some(true)` if received,
/// `Some(false)` for any other message, `None` if stream closed.
async fn wait_for_init(stream: &mut (impl StreamExt<Item = Result<Message, axum::Error>> + Unpin)) -> Option<bool> {
    while let Some(Ok(msg)) = stream.next().await {
        if let Some(client_msg) = parse_client_message(&msg) {
            return Some(matches!(client_msg, ClientMessage::ConnectionInit { .. }));
        }
        // Skip non-text messages during init.
    }
    None
}

/// Parse a WebSocket message into a `ClientMessage`.
fn parse_client_message(msg: &Message) -> Option<ClientMessage> {
    match msg {
        Message::Text(text) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

/// Serialize and send a `ServerMessage` over the WebSocket.
async fn send_message(sink: &mut (impl SinkExt<Message> + Unpin), msg: &ServerMessage) -> Result<(), ()> {
    let json = serde_json::to_string(msg).map_err(|_| ())?;
    sink.send(Message::Text(json.into())).await.map_err(|_| ())
}

/// Spawn a task that plans the subscription query, connects to the target
/// subgraph via WebSocket, and forwards messages to the client.
fn spawn_subscription(
    id: String,
    payload: SubscribePayload,
    state: Arc<AppState>,
    tx: mpsc::Sender<ServerMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Plan the query to find target subgraph.
        let Some(planner) = &state.planner else {
            let _ = tx
                .send(ServerMessage::Error {
                    id,
                    payload: vec![serde_json::json!({"message": "no supergraph loaded"})],
                })
                .await;
            return;
        };

        let plan = match planner.plan(&payload.query, payload.operation_name.as_deref()) {
            Ok(plan) => plan,
            Err(e) => {
                let _ = tx
                    .send(ServerMessage::Error {
                        id,
                        payload: vec![serde_json::json!({"message": format!("query planning failed: {e}")})],
                    })
                    .await;
                return;
            }
        };

        // Extract the target subgraph from the plan's root fetch node.
        let Some(subgraph_name) = extract_subscription_subgraph(&plan) else {
            let _ = tx
                .send(ServerMessage::Error {
                    id,
                    payload: vec![serde_json::json!({"message": "subscription must target a single subgraph"})],
                })
                .await;
            return;
        };

        let Some(subgraph_url) = state.subgraph_urls.get(&subgraph_name) else {
            let _ = tx
                .send(ServerMessage::Error {
                    id,
                    payload: vec![serde_json::json!({"message": format!("subgraph not found: {subgraph_name}")})],
                })
                .await;
            return;
        };

        // Convert HTTP URL to WebSocket URL.
        let ws_url = http_to_ws_url(subgraph_url);

        // Forward subscription to the upstream subgraph.
        if let Err(e) = upstream::forward_subscription(&id, &payload, &ws_url, &tx).await {
            tracing::warn!(error = %e, subgraph = %subgraph_name, "subscription forwarding failed");
            let _ = tx
                .send(ServerMessage::Error {
                    id,
                    payload: vec![serde_json::json!({"message": format!("subscription failed: {e}")})],
                })
                .await;
        }
    })
}

/// Extract the target subgraph name from a subscription query plan.
///
/// A subscription plan should have a Subscription node wrapping a Fetch,
/// or a direct Fetch node.
fn extract_subscription_subgraph(plan: &lattice_federation::QueryPlan) -> Option<String> {
    use lattice_federation::PlanNode;

    let node = plan.node.as_ref()?;
    match node {
        PlanNode::Subscription(sub) => {
            // The primary field is a Box<PlanNode> — should contain a Fetch.
            if let PlanNode::Fetch(fetch) = sub.primary.as_ref() {
                Some(fetch.service_name.clone())
            } else {
                None
            }
        }
        PlanNode::Fetch(fetch) => Some(fetch.service_name.clone()),
        _ => None,
    }
}

/// Convert an HTTP URL to a WebSocket URL (http→ws, https→wss).
fn http_to_ws_url(url: &str) -> String {
    url.strip_prefix("https://").map_or_else(
        || {
            url.strip_prefix("http://")
                .map_or_else(|| url.to_owned(), |rest| format!("ws://{rest}"))
        },
        |rest| format!("wss://{rest}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_to_ws() {
        assert_eq!(
            http_to_ws_url("http://localhost:3000/graphql"),
            "ws://localhost:3000/graphql"
        );
        assert_eq!(
            http_to_ws_url("https://api.example.com/graphql"),
            "wss://api.example.com/graphql"
        );
        assert_eq!(http_to_ws_url("ws://already-ws"), "ws://already-ws");
    }
}
