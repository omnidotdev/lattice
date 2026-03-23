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

/// Input sent to the `on_subgraph_request` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnSubgraphRequestInput {
    /// Name of the target subgraph.
    pub subgraph_name: String,
    /// Subgraph URL.
    pub url: String,
    /// The GraphQL query being sent.
    pub query: String,
    /// Query variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<serde_json::Value>,
    /// Request headers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

/// Input sent to the `on_subgraph_response` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnSubgraphResponseInput {
    /// Name of the responding subgraph.
    pub subgraph_name: String,
    /// Response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Response errors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<serde_json::Value>,
}

/// Input sent to the `on_operation_response` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnOperationResponseInput {
    /// The original query string.
    pub query: String,
    /// Operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    /// Response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Response errors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<serde_json::Value>,
}

/// Input sent to the `on_http_response` plugin hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnHttpResponseInput {
    /// HTTP status code.
    pub status_code: u16,
    /// Response body as JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
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

/// Input sent to the `on_graphql_parse` plugin hook.
///
/// Fired after parsing the GraphQL AST, before validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnGraphqlParseInput {
    /// The GraphQL query string.
    pub query: String,
    /// Optional operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
}

/// Input sent to the `on_graphql_validation` plugin hook.
///
/// Fired after validation, before query planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnGraphqlValidationInput {
    /// The GraphQL query string.
    pub query: String,
    /// Optional operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    /// Whether the query passed validation.
    pub is_valid: bool,
    /// Validation error messages (empty if valid).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_errors: Vec<String>,
}

/// Input sent to the `on_query_plan` plugin hook.
///
/// Fired after query planning, before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnQueryPlanInput {
    /// The GraphQL query string.
    pub query: String,
    /// Optional operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    /// Serialized query plan.
    pub plan_json: serde_json::Value,
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
    fn on_graphql_parse_input_serializes() {
        let input = OnGraphqlParseInput {
            query: "{ me { id } }".into(),
            operation_name: None,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("me"));
    }

    #[test]
    fn on_graphql_validation_input_serializes() {
        let input = OnGraphqlValidationInput {
            query: "{ me { id } }".into(),
            operation_name: None,
            is_valid: true,
            validation_errors: vec![],
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("is_valid"));
    }

    #[test]
    fn on_query_plan_input_serializes() {
        let input = OnQueryPlanInput {
            query: "{ me { id } }".into(),
            operation_name: None,
            plan_json: serde_json::json!({"node": null}),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("plan_json"));
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
