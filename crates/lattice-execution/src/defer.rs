use serde::Serialize;

/// A deferred patch to be sent as part of a multipart response.
#[derive(Debug, Clone, Serialize)]
pub struct DeferredPatch {
    /// JSON path to where this patch should be merged.
    pub path: Vec<String>,
    /// The label from the `@defer` directive (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The deferred data.
    pub data: serde_json::Value,
    /// Whether there are more patches to come.
    pub has_next: bool,
}

/// Encode a sequence of deferred patches as `multipart/mixed` body parts.
///
/// Each part is a JSON object with `path`, `label`, `data`, and `hasNext`.
#[must_use]
pub fn encode_multipart_patches(initial: &serde_json::Value, patches: &[DeferredPatch]) -> Vec<u8> {
    let boundary = "---";
    let mut buf = Vec::new();

    // Initial payload.
    let initial_json = serde_json::json!({
        "data": initial,
        "hasNext": !patches.is_empty(),
    });
    buf.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    buf.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    buf.extend_from_slice(serde_json::to_string(&initial_json).unwrap_or_default().as_bytes());

    // Deferred patches.
    for (i, patch) in patches.iter().enumerate() {
        let is_last = i == patches.len() - 1;
        let patch_json = serde_json::json!({
            "path": patch.path,
            "label": patch.label,
            "data": patch.data,
            "hasNext": !is_last,
        });
        buf.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
        buf.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
        buf.extend_from_slice(serde_json::to_string(&patch_json).unwrap_or_default().as_bytes());
    }

    // Terminating boundary.
    buf.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    buf
}

/// Check whether a GraphQL query string contains `@defer`.
#[must_use]
pub fn query_has_defer(query: &str) -> bool {
    query.contains("@defer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_defer_in_query() {
        assert!(query_has_defer("{ me { id ...@defer { email } } }"));
        assert!(!query_has_defer("{ me { id email } }"));
    }

    #[test]
    fn encode_multipart_initial_only() {
        let initial = json!({"me": {"id": "1"}});
        let body = encode_multipart_patches(&initial, &[]);
        let text = String::from_utf8(body).unwrap();

        assert!(text.contains("hasNext"));
        assert!(text.contains(r#""hasNext":false"#));
        assert!(text.contains("---"));
    }

    #[test]
    fn encode_multipart_with_patches() {
        let initial = json!({"me": {"id": "1"}});
        let patches = vec![DeferredPatch {
            path: vec!["me".into()],
            label: Some("details".into()),
            data: json!({"email": "alice@example.com"}),
            has_next: false,
        }];
        let body = encode_multipart_patches(&initial, &patches);
        let text = String::from_utf8(body).unwrap();

        // Should have two parts.
        assert_eq!(text.matches("Content-Type: application/json").count(), 2);
        assert!(text.contains("alice@example.com"));
    }

    #[test]
    fn has_next_set_correctly_in_patches() {
        let initial = json!({"me": {"id": "1"}});
        let patches = vec![
            DeferredPatch {
                path: vec!["me".into()],
                label: None,
                data: json!({"name": "Alice"}),
                has_next: true,
            },
            DeferredPatch {
                path: vec!["me".into()],
                label: None,
                data: json!({"email": "alice@example.com"}),
                has_next: false,
            },
        ];
        let body = encode_multipart_patches(&initial, &patches);
        let text = String::from_utf8(body).unwrap();

        // First part (initial) should have hasNext: true.
        assert!(text.contains(r#""hasNext":true"#));
        // Last patch should have hasNext: false.
        let last_part = text.rsplit("Content-Type: application/json").next().unwrap();
        assert!(last_part.contains(r#""hasNext":false"#));
    }
}
