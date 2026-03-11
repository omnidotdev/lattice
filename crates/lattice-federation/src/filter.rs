use std::collections::HashSet;

/// Filter a supergraph SDL by removing elements based on `@tag` directives.
///
/// # Errors
///
/// Returns an error if the SDL cannot be processed.
pub fn filter_sdl(
    sdl: &str,
    include_tags: &[String],
    exclude_tags: &[String],
) -> Result<String, crate::FederationError> {
    if include_tags.is_empty() && exclude_tags.is_empty() {
        return Ok(sdl.to_owned());
    }

    let include_set: HashSet<&str> = include_tags.iter().map(String::as_str).collect();
    let exclude_set: HashSet<&str> = exclude_tags.iter().map(String::as_str).collect();

    let mut output_lines: Vec<&str> = Vec::new();
    let mut skip_type = false;
    let mut brace_depth: i32 = 0;
    let mut in_type_block = false;

    for line in sdl.lines() {
        let trimmed = line.trim();

        // Track type-level definitions.
        if is_type_definition(trimmed) {
            let tags = extract_tags(trimmed);
            // Only cascade-skip a type if it has tags that cause exclusion.
            // Untagged types pass through so their fields can be individually filtered.
            if !tags.is_empty() && should_exclude(&tags, &include_set, &exclude_set) {
                skip_type = true;
                brace_depth = 0;
                if trimmed.contains('{') {
                    brace_depth = 1;
                }
                continue;
            }
            in_type_block = trimmed.contains('{');
            skip_type = false;
        }

        // Handle brace tracking for skipped types.
        if skip_type {
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            if brace_depth <= 0 {
                skip_type = false;
                in_type_block = false;
            }
            continue;
        }

        // Inside a type block, check field-level tags.
        if in_type_block && is_field_line(trimmed) {
            let tags = extract_tags(trimmed);
            if !tags.is_empty() && should_exclude(&tags, &include_set, &exclude_set) {
                continue;
            }
            // In include-only mode, untagged fields are removed.
            if !include_set.is_empty() && tags.is_empty() {
                continue;
            }
        }

        // Track brace depth for type blocks.
        for ch in trimmed.chars() {
            match ch {
                '{' => {
                    brace_depth += 1;
                    in_type_block = true;
                }
                '}' => {
                    brace_depth -= 1;
                    if brace_depth <= 0 {
                        in_type_block = false;
                    }
                }
                _ => {}
            }
        }

        output_lines.push(line);
    }

    Ok(output_lines.join("\n"))
}

/// Check if a line starts a type definition (but not root types).
fn is_type_definition(line: &str) -> bool {
    let starts_type = line.starts_with("type ")
        || line.starts_with("input ")
        || line.starts_with("enum ")
        || line.starts_with("interface ")
        || line.starts_with("union ")
        || line.starts_with("scalar ");

    if !starts_type {
        return false;
    }

    // Don't filter root Query/Mutation/Subscription types themselves.
    // Also don't filter federation internal types.
    !line.starts_with("type Query")
        && !line.starts_with("type Mutation")
        && !line.starts_with("type Subscription")
        && !line.starts_with("enum join__Graph")
        && !line.starts_with("enum link__Purpose")
        && !line.starts_with("scalar join__")
        && !line.starts_with("scalar link__")
}

/// Check if a line looks like a field definition inside a type block.
fn is_field_line(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('}')
        && !trimmed.starts_with('{')
        && !trimmed.starts_with('#')
        && !trimmed.starts_with("directive")
        && !trimmed.starts_with("schema")
        && !trimmed.starts_with("type ")
        && !trimmed.starts_with("enum ")
        && !trimmed.starts_with("input ")
        && !trimmed.starts_with("interface ")
        && !trimmed.starts_with("union ")
        && !trimmed.starts_with("scalar ")
}

/// Extract tag names from `@tag(name: "value")` directives on a line.
fn extract_tags(line: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut search = line;

    while let Some(pos) = search.find("@tag(") {
        let rest = &search[pos + 5..];
        if let Some(name_pos) = rest.find("name:") {
            let after_name = rest[name_pos + 5..].trim_start();
            if let Some(quote_start) = after_name.find('"') {
                let after_quote = &after_name[quote_start + 1..];
                if let Some(quote_end) = after_quote.find('"') {
                    tags.push(after_quote[..quote_end].to_owned());
                }
            }
        }
        search = &search[pos + 5..];
    }

    tags
}

/// Determine if an element should be excluded based on its tags.
fn should_exclude(tags: &[String], include_set: &HashSet<&str>, exclude_set: &HashSet<&str>) -> bool {
    // Check exclude list.
    if tags.iter().any(|t| exclude_set.contains(t.as_str())) {
        return true;
    }

    // Check include list.
    if !include_set.is_empty() && !tags.iter().any(|t| include_set.contains(t.as_str())) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGGED_SDL: &str = r#"
        schema
            @link(url: "https://specs.apollo.dev/link/v1.0")
            @link(url: "https://specs.apollo.dev/join/v0.3", for: EXECUTION)
        {
            query: Query
        }

        directive @tag(name: String!) repeatable on FIELD_DEFINITION | OBJECT | INTERFACE | UNION | ENUM | ENUM_VALUE | INPUT_OBJECT | INPUT_FIELD_DEFINITION | ARGUMENT_DEFINITION | SCALAR

        directive @join__enumValue(graph: join__Graph!) repeatable on ENUM_VALUE
        directive @join__field(graph: join__Graph, requires: join__FieldSet, provides: join__FieldSet, type: String, external: Boolean, override: String, usedOverridden: Boolean) repeatable on FIELD_DEFINITION | INPUT_FIELD_DEFINITION
        directive @join__graph(name: String!, url: String!) on ENUM_VALUE
        directive @join__implements(graph: join__Graph!, interface: String!) repeatable on OBJECT | INTERFACE
        directive @join__type(graph: join__Graph!, key: join__FieldSet, extension: Boolean! = false, resolvable: Boolean! = true, isInterfaceObject: Boolean! = false) repeatable on OBJECT | INTERFACE | UNION | ENUM | INPUT_OBJECT | SCALAR
        directive @join__unionMember(graph: join__Graph!, member: String!) repeatable on UNION
        directive @link(url: String, as: String, for: link__Purpose, import: [link__Import]) repeatable on SCHEMA

        scalar join__FieldSet
        scalar link__Import
        enum link__Purpose { SECURITY EXECUTION }

        enum join__Graph {
            USERS @join__graph(name: "users", url: "http://users:3000/graphql")
        }

        type Query @join__type(graph: USERS) {
            me: User @join__field(graph: USERS)
            adminStats: AdminStats @join__field(graph: USERS) @tag(name: "internal")
        }

        type User @join__type(graph: USERS, key: "id") {
            id: ID!
            name: String
            email: String @tag(name: "internal")
            ssn: String @tag(name: "internal") @tag(name: "admin")
        }

        type AdminStats @tag(name: "internal") {
            totalUsers: Int
            activeUsers: Int
        }
    "#;

    #[test]
    fn exclude_removes_tagged_fields() {
        let result = filter_sdl(TAGGED_SDL, &[], &["internal".into()]).unwrap();
        assert!(!result.contains("email"), "email should be excluded");
        assert!(!result.contains("ssn"), "ssn should be excluded");
        assert!(!result.contains("adminStats"), "adminStats field should be excluded");
        assert!(!result.contains("AdminStats"), "AdminStats type should be excluded");
        assert!(result.contains("name"), "name should remain");
        assert!(result.contains("id"), "id should remain");
    }

    #[test]
    fn exclude_multiple_tags() {
        let result = filter_sdl(TAGGED_SDL, &[], &["admin".into()]).unwrap();
        assert!(!result.contains("ssn"), "ssn has @tag(name: \"admin\")");
        assert!(result.contains("email"), "email only has internal tag");
    }

    #[test]
    fn include_and_exclude_combined() {
        let result = filter_sdl(TAGGED_SDL, &["internal".into()], &["admin".into()]).unwrap();
        assert!(result.contains("email"), "email has internal, not admin");
        assert!(!result.contains("ssn"), "ssn has admin tag -> excluded");
    }

    #[test]
    fn empty_filters_returns_unmodified() {
        let result = filter_sdl(TAGGED_SDL, &[], &[]).unwrap();
        assert!(result.contains("email"));
        assert!(result.contains("ssn"));
        assert!(result.contains("adminStats"));
        assert!(result.contains("name"));
    }

    #[test]
    fn type_level_tag_cascades() {
        let result = filter_sdl(TAGGED_SDL, &[], &["internal".into()]).unwrap();
        assert!(!result.contains("AdminStats"), "type excluded");
        assert!(!result.contains("totalUsers"), "field of excluded type");
        assert!(!result.contains("activeUsers"), "field of excluded type");
    }

    #[test]
    fn preserves_federation_types() {
        let result = filter_sdl(TAGGED_SDL, &[], &["internal".into()]).unwrap();
        assert!(result.contains("join__Graph"), "federation enum preserved");
        assert!(result.contains("join__FieldSet"), "federation scalar preserved");
        assert!(result.contains("link__Purpose"), "link enum preserved");
    }

    #[test]
    fn preserves_query_type() {
        let result = filter_sdl(TAGGED_SDL, &[], &["internal".into()]).unwrap();
        assert!(result.contains("type Query"), "Query type preserved");
        assert!(result.contains("me:"), "untagged Query field preserved");
    }
}
