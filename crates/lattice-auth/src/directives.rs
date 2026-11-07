use crate::tree::{AuthRequirement, AuthTree};

/// Parse `@requiresAuthentication` and `@requiresScopes` directives from
/// supergraph SDL and build an `AuthTree`.
///
/// This is a lightweight line-based parser that extracts directive annotations
/// from type and field definitions. It doesn't need a full GraphQL parser
/// because the supergraph SDL follows a predictable format.
#[must_use]
pub fn build_auth_tree(sdl: &str, check_scopes: bool, check_auth: bool) -> AuthTree {
    let mut tree = AuthTree::new();

    if !check_scopes && !check_auth {
        return tree;
    }

    let mut current_type: Option<String> = None;

    for line in sdl.lines() {
        let trimmed = line.trim();

        // Track type context.
        if let Some(type_name) = extract_type_name(trimmed) {
            // Check for type-level directives.
            let mut req = AuthRequirement::default();

            if check_auth && trimmed.contains("@requiresAuthentication") {
                req.requires_auth = true;
            }
            if check_scopes {
                if let Some(scopes) = extract_requires_scopes(trimmed) {
                    req.scopes = scopes;
                }
            }

            if !req.is_empty() {
                tree.insert(type_name.clone(), req);
            }

            current_type = Some(type_name);
            continue;
        }

        // End of type block.
        if trimmed == "}" {
            current_type = None;
            continue;
        }

        // Field-level directives.
        if let Some(ref type_name) = current_type {
            if let Some(field_name) = extract_field_name(trimmed) {
                let mut req = AuthRequirement::default();

                if check_auth && trimmed.contains("@requiresAuthentication") {
                    req.requires_auth = true;
                }
                if check_scopes {
                    if let Some(scopes) = extract_requires_scopes(trimmed) {
                        req.scopes = scopes;
                    }
                }

                if !req.is_empty() {
                    tree.insert(format!("{type_name}.{field_name}"), req);
                }
            }
        }
    }

    tree
}

/// Extract type name from a line like `type User @join__type(...)`.
fn extract_type_name(line: &str) -> Option<String> {
    let line = line.strip_prefix("type ")?;
    let name: String = line.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}

/// Extract field name from a line like `  email: String @requiresScopes(...)`.
fn extract_field_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with('@') || trimmed.is_empty() {
        return None;
    }

    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || !trimmed[name.len()..].trim_start().starts_with(':') {
        return None;
    }

    Some(name)
}

/// Parse `@requiresScopes(scopes: [["a", "b"], ["c"]])` from a line.
///
/// Returns the scopes as OR-groups of AND-scopes.
fn extract_requires_scopes(line: &str) -> Option<Vec<Vec<String>>> {
    let marker = "@requiresScopes(scopes:";
    let start = line.find(marker)?;
    let rest = &line[start + marker.len()..];
    let rest = rest.trim_start();

    // Find the balanced outer brackets.
    let mut depth = 0;
    let mut end = 0;
    for (i, ch) in rest.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == 0 {
        return None;
    }

    let array_str = &rest[..end];
    // Parse the JSON-like array.
    let parsed: Vec<Vec<String>> = serde_json::from_str(array_str).ok()?;
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requires_authentication_on_type() {
        let sdl = r"
type User @requiresAuthentication {
    id: ID!
    name: String
}
";
        let tree = build_auth_tree(sdl, false, true);
        let req = tree.requirement("User", "id").unwrap();
        assert!(req.requires_auth);
    }

    #[test]
    fn parses_requires_scopes_on_field() {
        let sdl = r#"
type User {
    id: ID!
    email: String @requiresScopes(scopes: [["read:email"]])
}
"#;
        let tree = build_auth_tree(sdl, true, false);
        assert!(tree.requirement("User", "id").is_none());
        let req = tree.requirement("User", "email").unwrap();
        assert_eq!(req.scopes, vec![vec!["read:email".to_string()]]);
    }

    #[test]
    fn parses_combined_directives() {
        let sdl = r#"
type Admin @requiresAuthentication {
    id: ID!
    secrets: String @requiresScopes(scopes: [["admin"]])
}
"#;
        let tree = build_auth_tree(sdl, true, true);
        // id inherits type-level @requiresAuthentication.
        let req = tree.requirement("Admin", "id").unwrap();
        assert!(req.requires_auth);
        // secrets has its own field-level scope.
        let req = tree.requirement("Admin", "secrets").unwrap();
        assert_eq!(req.scopes, vec![vec!["admin".to_string()]]);
    }

    #[test]
    fn returns_empty_tree_when_disabled() {
        let sdl = r"
type User @requiresAuthentication {
    id: ID!
}
";
        let tree = build_auth_tree(sdl, false, false);
        assert!(tree.is_empty());
    }

    #[test]
    fn parses_or_scope_groups() {
        let sdl = r#"
type Query {
    users: [User] @requiresScopes(scopes: [["read:users", "read:org"], ["admin"]])
}
"#;
        let tree = build_auth_tree(sdl, true, false);
        let req = tree.requirement("Query", "users").unwrap();
        assert_eq!(req.scopes.len(), 2);
    }

    #[test]
    fn extract_field_name_works() {
        assert_eq!(extract_field_name("  email: String"), Some("email".into()));
        assert_eq!(extract_field_name("  id: ID!"), Some("id".into()));
        assert_eq!(extract_field_name("  # comment"), None);
        assert_eq!(extract_field_name("  @directive"), None);
    }

    #[test]
    fn extract_type_name_works() {
        assert_eq!(
            extract_type_name("type User @join__type(graph: USERS)"),
            Some("User".into())
        );
        assert_eq!(extract_type_name("type Query"), Some("Query".into()));
        assert_eq!(extract_type_name("enum Something"), None);
    }
}
