use serde::{Deserialize, Serialize};

/// Messages sent from the client to the server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // Protocol fields must be deserialized even if not read.
pub enum ClientMessage {
    /// Initialize the connection (must be first message).
    ConnectionInit {
        #[serde(default)]
        payload: Option<serde_json::Value>,
    },
    /// Subscribe to an operation.
    Subscribe { id: String, payload: SubscribePayload },
    /// Stop listening to an operation.
    Complete { id: String },
    /// Heartbeat probe.
    Ping {
        #[serde(default)]
        payload: Option<serde_json::Value>,
    },
    /// Heartbeat response.
    Pong {
        #[serde(default)]
        payload: Option<serde_json::Value>,
    },
}

/// The payload of a Subscribe message.
#[derive(Debug, Deserialize)]
pub struct SubscribePayload {
    pub query: String,
    #[serde(default, rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
}

/// Messages sent from the server to the client.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Acknowledge connection initialization.
    ConnectionAck {
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<serde_json::Value>,
    },
    /// Deliver an operation result.
    Next { id: String, payload: serde_json::Value },
    /// Report operation errors (terminates the operation).
    Error {
        id: String,
        payload: Vec<serde_json::Value>,
    },
    /// Signal operation completion.
    Complete { id: String },
    /// Heartbeat response.
    Pong {
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<serde_json::Value>,
    },
}

/// WebSocket close codes defined by the graphql-transport-ws protocol.
pub mod close_code {
    /// Invalid message format.
    pub const INVALID_MESSAGE: u16 = 4400;
    /// Client did not send `ConnectionInit` before subscribing.
    pub const UNAUTHORIZED: u16 = 4401;
    /// Connection initialization timeout.
    pub const INIT_TIMEOUT: u16 = 4408;
    /// Duplicate subscriber ID.
    pub const SUBSCRIBER_ALREADY_EXISTS: u16 = 4409;
    /// Too many initialization requests.
    pub const TOO_MANY_INIT_REQUESTS: u16 = 4429;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_connection_init() {
        let json = r#"{"type": "connection_init"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::ConnectionInit { payload: None }));
    }

    #[test]
    fn parse_connection_init_with_payload() {
        let json = r#"{"type": "connection_init", "payload": {"token": "abc"}}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::ConnectionInit { payload: Some(_) }));
    }

    #[test]
    fn parse_subscribe() {
        let json = r#"{"type": "subscribe", "id": "1", "payload": {"query": "subscription { onEvent }"}}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::Subscribe { id, .. } if id == "1"));
    }

    #[test]
    fn parse_complete() {
        let json = r#"{"type": "complete", "id": "1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::Complete { id } if id == "1"));
    }

    #[test]
    fn parse_ping() {
        let json = r#"{"type": "ping"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::Ping { payload: None }));
    }

    #[test]
    fn serialize_connection_ack() {
        let msg = ServerMessage::ConnectionAck { payload: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"connection_ack"}"#);
    }

    #[test]
    fn serialize_next() {
        let msg = ServerMessage::Next {
            id: "1".into(),
            payload: serde_json::json!({"data": {"onEvent": "hello"}}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"next""#));
        assert!(json.contains(r#""id":"1""#));
    }

    #[test]
    fn serialize_error() {
        let msg = ServerMessage::Error {
            id: "1".into(),
            payload: vec![serde_json::json!({"message": "bad query"})],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
    }

    #[test]
    fn serialize_complete() {
        let msg = ServerMessage::Complete { id: "1".into() };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"complete","id":"1"}"#);
    }

    #[test]
    fn serialize_pong() {
        let msg = ServerMessage::Pong { payload: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"pong"}"#);
    }
}
