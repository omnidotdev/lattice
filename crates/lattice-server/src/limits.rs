use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use graphql_tools::parser::query::{Definition, Document, OperationDefinition, Selection, SelectionSet};
use http_body_util::BodyExt;

use lattice_config::LimitsConfig;

use crate::SharedState;

/// Body size limit middleware.
///
/// Read the request body, enforce the configured max size, then re-attach
/// the body so downstream handlers can consume it. Runs before GraphQL
/// parsing to reject oversized payloads early.
pub async fn body_size_limit(State(shared): State<SharedState>, request: Request<Body>, next: Next) -> Response {
    let state = shared.load_full();

    let Some(limits) = &state.config.limits else {
        return next.run(request).await;
    };

    let max_bytes = parse_size(&limits.max_body_size);

    let (parts, body) = request.into_parts();

    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "failed to read request body"),
    };

    if bytes.len() > max_bytes {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "request body exceeds limit: {} bytes > {} bytes",
                bytes.len(),
                max_bytes
            ),
        );
    }

    let request = Request::from_parts(parts, Body::from(bytes));
    next.run(request).await
}

/// Validate a GraphQL query string against depth, complexity, and alias limits.
///
/// Returns `None` if the query passes all checks, or `Some(error_message)` if
/// a limit is exceeded.
pub fn validate_query(query: &str, config: &LimitsConfig) -> Option<String> {
    let doc: Document<'_, String> = match graphql_tools::parser::parse_query::<String>(query) {
        Ok(doc) => doc,
        Err(_) => return None, // Let the planner produce the real parse error.
    };

    let mut max_depth: usize = 0;
    let mut total_fields: usize = 0;
    let mut alias_count: usize = 0;

    for def in &doc.definitions {
        let sel_set = match def {
            Definition::Operation(op) => match op {
                OperationDefinition::SelectionSet(sel) => Some(sel),
                OperationDefinition::Query(q) => Some(&q.selection_set),
                OperationDefinition::Mutation(m) => Some(&m.selection_set),
                OperationDefinition::Subscription(s) => Some(&s.selection_set),
            },
            Definition::Fragment(f) => Some(&f.selection_set),
        };

        if let Some(sel) = sel_set {
            walk_selection_set(sel, 1, &mut max_depth, &mut total_fields, &mut alias_count);
        }
    }

    if max_depth > config.max_depth {
        return Some(format!("query depth {max_depth} exceeds limit {}", config.max_depth));
    }

    if total_fields > config.max_complexity {
        return Some(format!(
            "query complexity {total_fields} exceeds limit {}",
            config.max_complexity
        ));
    }

    if alias_count > config.max_aliases {
        return Some(format!(
            "query alias count {alias_count} exceeds limit {}",
            config.max_aliases
        ));
    }

    None
}

/// Recursively walk a selection set, tracking depth, field count, and alias usage.
fn walk_selection_set(
    selection_set: &SelectionSet<'_, String>,
    current_depth: usize,
    max_depth: &mut usize,
    total_fields: &mut usize,
    alias_count: &mut usize,
) {
    if current_depth > *max_depth {
        *max_depth = current_depth;
    }

    for selection in &selection_set.items {
        match selection {
            Selection::Field(field) => {
                *total_fields += 1;

                if field.alias.is_some() {
                    *alias_count += 1;
                }

                if !field.selection_set.items.is_empty() {
                    walk_selection_set(
                        &field.selection_set,
                        current_depth + 1,
                        max_depth,
                        total_fields,
                        alias_count,
                    );
                }
            }
            Selection::InlineFragment(frag) => {
                walk_selection_set(&frag.selection_set, current_depth, max_depth, total_fields, alias_count);
            }
            Selection::FragmentSpread(_) => {
                // Fragment spreads are resolved at execution time; skip here.
            }
        }
    }
}

/// Parse a human-readable size string into bytes.
///
/// Supports `kb`, `mb`, `gb` suffixes (case-insensitive) and plain byte counts.
fn parse_size(s: &str) -> usize {
    let s = s.trim().to_lowercase();

    if let Some(mb) = s.strip_suffix("mb") {
        return mb.trim().parse::<usize>().unwrap_or(2) * 1024 * 1024;
    }
    if let Some(kb) = s.strip_suffix("kb") {
        return kb.trim().parse::<usize>().unwrap_or(256) * 1024;
    }
    if let Some(gb) = s.strip_suffix("gb") {
        return gb.trim().parse::<usize>().unwrap_or(1) * 1024 * 1024 * 1024;
    }

    // Plain byte count.
    s.parse().unwrap_or(2 * 1024 * 1024)
}

/// Build a JSON error response matching the GraphQL error shape.
fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({
        "errors": [{ "message": message }]
    });

    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_megabytes() {
        assert_eq!(parse_size("2mb"), 2 * 1024 * 1024);
        assert_eq!(parse_size("10MB"), 10 * 1024 * 1024);
    }

    #[test]
    fn parse_size_kilobytes() {
        assert_eq!(parse_size("256kb"), 256 * 1024);
    }

    #[test]
    fn parse_size_plain_bytes() {
        assert_eq!(parse_size("1048576"), 1_048_576);
    }

    fn default_limits() -> LimitsConfig {
        LimitsConfig {
            max_depth: 3,
            max_complexity: 10,
            max_body_size: "2mb".into(),
            max_aliases: 2,
        }
    }

    #[test]
    fn shallow_query_passes() {
        let query = "{ me { name } }";
        assert!(validate_query(query, &default_limits()).is_none());
    }

    #[test]
    fn depth_limit_exceeded() {
        // Depth 4: root > a > b > c > d.
        let query = "{ a { b { c { d } } } }";
        let result = validate_query(query, &default_limits());
        assert!(result.is_some());
        assert!(result.unwrap().contains("depth"));
    }

    #[test]
    fn alias_limit_exceeded() {
        let query = "{ a1: me { name } a2: me { name } a3: me { name } }";
        let result = validate_query(query, &default_limits());
        assert!(result.is_some());
        assert!(result.unwrap().contains("alias"));
    }

    #[test]
    fn complexity_limit_exceeded() {
        let limits = LimitsConfig {
            max_depth: 100,
            max_complexity: 3,
            max_body_size: "2mb".into(),
            max_aliases: 100,
        };
        let query = "{ a b c d }";
        let result = validate_query(query, &limits);
        assert!(result.is_some());
        assert!(result.unwrap().contains("complexity"));
    }
}
