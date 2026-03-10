use serde::{Deserialize, Serialize};

/// Input sent to the `on_request` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnRequestInput {
    /// The GraphQL query string.
    pub query: String,
    /// Optional operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    /// Optional variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<serde_json::Value>,
    /// Client IP from x-forwarded-for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
}

/// Input sent to the `on_response` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnResponseInput {
    /// The original query string.
    pub query: String,
    /// The response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// The response errors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<serde_json::Value>,
}

/// Result returned by plugin hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HookResult {
    /// Continue processing unchanged.
    Continue,
    /// Modify the request or response data.
    Modify {
        /// Modified data payload (meaning depends on hook type).
        data: serde_json::Value,
    },
    /// Reject the request with an error message.
    Reject {
        /// Human-readable rejection reason.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_result_continue_serializes() {
        let r = HookResult::Continue;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"action":"continue"}"#);

        let deserialized: HookResult = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, HookResult::Continue));
    }

    #[test]
    fn hook_result_reject_serializes() {
        let r = HookResult::Reject {
            message: "blocked".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""action":"reject""#));
        assert!(json.contains(r#""message":"blocked""#));

        let deserialized: HookResult = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, HookResult::Reject { message } if message == "blocked"));
    }

    #[test]
    fn hook_result_modify_serializes() {
        let r = HookResult::Modify {
            data: serde_json::json!({"query": "{ me }"}),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""action":"modify""#));

        let deserialized: HookResult = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, HookResult::Modify { .. }));
    }

    #[test]
    fn on_request_input_serializes() {
        let input = OnRequestInput {
            query: "{ me { id } }".into(),
            operation_name: Some("Me".into()),
            variables: None,
            client_ip: Some("1.2.3.4".into()),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("me"));
        // variables is None so should be skipped
        assert!(!json.contains("variables"));
    }

    #[test]
    fn on_response_input_serializes() {
        let input = OnResponseInput {
            query: "{ me }".into(),
            data: Some(serde_json::json!({"me": {"id": "1"}})),
            errors: vec![],
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("me"));
        // errors is empty so should be skipped
        assert!(!json.contains("errors"));
    }
}
