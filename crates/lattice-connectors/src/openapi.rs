//! OpenAPI ingestion: turn an OpenAPI 3.x spec into a federated REST subgraph.
//!
//! [`generate`] reads a spec and produces two artifacts the rest of the gateway
//! already consumes: a GraphQL SDL (for composition into the supergraph) and a
//! [`RestConnectorConfig`] (the per-field REST mappings the [`crate::rest`]
//! executor runs at request time). No schema or mappings are hand-written: each
//! OpenAPI operation becomes a root field, and each referenced schema becomes a
//! GraphQL type.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use lattice_config::{RestConnectorConfig, RestFieldMapping};
use serde_json::Value;
use thiserror::Error;

/// HTTP methods, in a stable order, mapped to whether they are a mutation.
const METHODS: [(&str, bool); 5] = [
    ("get", false),
    ("post", true),
    ("put", true),
    ("patch", true),
    ("delete", true),
];

/// Errors raised while generating a subgraph from an OpenAPI spec.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpenApiError {
    /// The spec root was not a JSON object.
    #[error("OpenAPI spec must be a JSON object")]
    NotAnObject,
    /// The spec had no `paths` object, so there are no operations to expose.
    #[error("OpenAPI spec has no paths")]
    NoPaths,
}

/// A subgraph generated from an OpenAPI spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSubgraph {
    /// GraphQL SDL describing the generated types and root fields.
    pub sdl: String,
    /// REST connector config (base URL and per-field mappings) for the executor.
    pub connector: RestConnectorConfig,
}

/// Convert a raw identifier into a valid GraphQL name.
///
/// GraphQL names match `[_A-Za-z][_0-9A-Za-z]*`. Invalid characters become
/// underscores and a leading digit is prefixed with one.
fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, ch) in raw.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if i == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// Map an OpenAPI primitive (`type`, optional `format`) to a GraphQL scalar.
fn scalar_for(ty: &str, format: Option<&str>) -> &'static str {
    match ty {
        "integer" => match format {
            // 64-bit integers overflow GraphQL's 32-bit Int, so widen to Float
            Some("int64") => "Float",
            _ => "Int",
        },
        "number" => "Float",
        "boolean" => "Boolean",
        // string and everything unrecognized fall back to String
        _ => "String",
    }
}

/// The last path segment of a JSON Pointer `$ref`, or `None` if not a local ref.
fn ref_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.rsplit('/').next())
}

/// Look up `components/schemas/<name>` in the spec.
fn component<'a>(spec: &'a Value, name: &str) -> Option<&'a Value> {
    spec.get("components")?.get("schemas")?.get(name)
}

/// Working state threaded through type resolution: which component schemas are
/// referenced (so they get emitted) and whether the `JSON` scalar is needed.
#[derive(Default)]
struct Usage {
    output_types: BTreeSet<String>,
    input_types: BTreeSet<String>,
    needs_json: bool,
}

/// Resolve a schema to a GraphQL output type reference (without a trailing `!`).
fn output_ref(schema: &Value, usage: &mut Usage) -> String {
    if let Some(name) = ref_name(schema) {
        let name = sanitize_name(name);
        usage.output_types.insert(name.clone());
        return name;
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => {
            let items = schema.get("items");
            // an array with no item schema degrades to a JSON scalar
            if items.is_none() {
                usage.needs_json = true;
            }
            let inner = items.map_or_else(|| "JSON".to_string(), |it| output_ref(it, usage));
            format!("[{inner}]")
        }
        Some(ty @ ("integer" | "number" | "boolean" | "string")) => {
            scalar_for(ty, schema.get("format").and_then(Value::as_str)).to_string()
        }
        // inline object (no $ref to name it) or an unknown shape: fall back to
        // a free-form JSON scalar rather than inventing a type name
        _ => {
            usage.needs_json = true;
            "JSON".to_string()
        }
    }
}

/// Resolve a schema to a GraphQL input type reference (without a trailing `!`).
///
/// Object `$ref`s become `<Name>Input` so input and output types never collide.
fn input_ref(schema: &Value, usage: &mut Usage) -> String {
    if let Some(name) = ref_name(schema) {
        let name = sanitize_name(name);
        usage.input_types.insert(name.clone());
        return format!("{name}Input");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => {
            let items = schema.get("items");
            if items.is_none() {
                usage.needs_json = true;
            }
            let inner = items.map_or_else(|| "JSON".to_string(), |it| input_ref(it, usage));
            format!("[{inner}]")
        }
        Some(ty @ ("integer" | "number" | "boolean" | "string")) => {
            scalar_for(ty, schema.get("format").and_then(Value::as_str)).to_string()
        }
        _ => {
            usage.needs_json = true;
            "JSON".to_string()
        }
    }
}

/// The set of required property names for an object schema.
fn required_set(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).map(ToString::to_string).collect())
        .unwrap_or_default()
}

/// Render the fields of an object schema as GraphQL field/arg lines, using the
/// given reference resolver (output or input).
fn render_fields(schema: &Value, usage: &mut Usage, resolve: fn(&Value, &mut Usage) -> String) -> Vec<String> {
    let required = required_set(schema);
    let mut lines = Vec::new();
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (name, prop) in props {
            let mut ty = resolve(prop, usage);
            if required.contains(name) {
                ty.push('!');
            }
            lines.push(format!("  {}: {}", sanitize_name(name), ty));
        }
    }
    lines
}

/// Pick the response schema for an operation: the first 2xx (or `default`) JSON
/// response body, if any.
fn response_schema(op: &Value) -> Option<&Value> {
    let responses = op.get("responses")?.as_object()?;
    responses
        .iter()
        .filter(|(code, _)| code.starts_with('2') || code.as_str() == "default")
        .min_by_key(|(code, _)| (*code).clone())
        .and_then(|(_, resp)| resp.get("content")?.get("application/json")?.get("schema"))
}

/// Build a field definition and its REST mapping for a single operation.
fn build_operation(path: &str, method: &str, op: &Value, usage: &mut Usage) -> (String, String, RestFieldMapping) {
    let field_name = op
        .get("operationId")
        .and_then(Value::as_str)
        .map_or_else(|| synth_field_name(method, path), sanitize_name);

    let mut args: Vec<String> = Vec::new();
    let mut query: BTreeMap<String, String> = BTreeMap::new();

    if let Some(params) = op.get("parameters").and_then(Value::as_array) {
        for p in params {
            let Some(pname) = p.get("name").and_then(Value::as_str) else {
                continue;
            };
            let location = p.get("in").and_then(Value::as_str).unwrap_or_default();
            let schema = p.get("schema").cloned().unwrap_or(Value::Null);
            let ty = output_ref(&schema, usage);
            let arg = sanitize_name(pname);
            match location {
                "path" => args.push(format!("{arg}: {ty}!")),
                "query" => {
                    args.push(format!("{arg}: {ty}"));
                    query.insert(pname.to_string(), arg);
                }
                _ => {}
            }
        }
    }

    let mut body_arg = None;
    if let Some(body_schema) = op
        .get("requestBody")
        .and_then(|b| b.get("content"))
        .and_then(|c| c.get("application/json"))
        .and_then(|j| j.get("schema"))
    {
        let ty = input_ref(body_schema, usage);
        let required = op
            .get("requestBody")
            .and_then(|b| b.get("required"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        args.push(format!("input: {ty}{}", if required { "!" } else { "" }));
        body_arg = Some("input".to_string());
    }

    let resp = response_schema(op);
    // an operation with no JSON response body degrades to a JSON scalar
    if resp.is_none() {
        usage.needs_json = true;
    }
    let ret = resp.map_or_else(|| "JSON".to_string(), |s| output_ref(s, usage));

    let arglist = if args.is_empty() {
        String::new()
    } else {
        format!("({})", args.join(", "))
    };
    let field_sdl = format!("  {field_name}{arglist}: {ret}");

    let mapping = RestFieldMapping {
        method: method.to_uppercase(),
        path: path.to_string(),
        query: query.into_iter().collect(),
        select: None,
        body: body_arg,
    };

    (field_name, field_sdl, mapping)
}

/// Synthesize a field name from an HTTP method and path when no `operationId`
/// is present, for example `get` + `/pets/{petId}` becomes `getPetsPetId` (the
/// path segments are camel-cased and appended to the method).
fn synth_field_name(method: &str, path: &str) -> String {
    let mut name = method.to_string();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let seg = seg.trim_start_matches('{').trim_end_matches('}');
        let mut chars = sanitize_name(seg).chars().collect::<Vec<_>>();
        if let Some(first) = chars.first_mut() {
            *first = first.to_ascii_uppercase();
        }
        name.extend(chars);
    }
    name
}

/// Emit a GraphQL `type`/`input` block for a component schema, pulling in any
/// further component schemas it references (transitive closure).
fn emit_types(spec: &Value, usage: &mut Usage) -> String {
    let mut out = String::new();
    let mut emitted_out: BTreeSet<String> = BTreeSet::new();
    let mut emitted_in: BTreeSet<String> = BTreeSet::new();

    // Output types (transitive)
    while let Some(name) = usage.output_types.iter().find(|n| !emitted_out.contains(*n)).cloned() {
        emitted_out.insert(name.clone());
        if let Some(schema) = component(spec, &name) {
            let lines = render_fields(schema, usage, output_ref);
            if lines.is_empty() {
                let _ = write!(out, "type {name} {{\n  _empty: Boolean\n}}\n\n");
            } else {
                let _ = write!(out, "type {name} {{\n{}\n}}\n\n", lines.join("\n"));
            }
        }
    }

    // Input types (transitive)
    while let Some(name) = usage.input_types.iter().find(|n| !emitted_in.contains(*n)).cloned() {
        emitted_in.insert(name.clone());
        if let Some(schema) = component(spec, &name) {
            let lines = render_fields(schema, usage, input_ref);
            if lines.is_empty() {
                let _ = write!(out, "input {name}Input {{\n  _empty: Boolean\n}}\n\n");
            } else {
                let _ = write!(out, "input {name}Input {{\n{}\n}}\n\n", lines.join("\n"));
            }
        }
    }

    out
}

/// Generate a [`GeneratedSubgraph`] from an OpenAPI 3.x spec.
///
/// # Errors
///
/// Returns [`OpenApiError`] when the spec is not an object or has no paths.
pub fn generate(spec: &Value) -> Result<GeneratedSubgraph, OpenApiError> {
    let obj = spec.as_object().ok_or(OpenApiError::NotAnObject)?;
    let paths = obj
        .get("paths")
        .and_then(Value::as_object)
        .ok_or(OpenApiError::NoPaths)?;

    let base_url = spec
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut usage = Usage::default();
    let mut query_fields: BTreeMap<String, String> = BTreeMap::new();
    let mut mutation_fields: BTreeMap<String, String> = BTreeMap::new();
    let mut fields: BTreeMap<String, RestFieldMapping> = BTreeMap::new();

    for (path, item) in paths {
        for (method, is_mutation) in METHODS {
            let Some(op) = item.get(method) else { continue };
            let (name, field_sdl, mapping) = build_operation(path, method, op, &mut usage);
            if is_mutation {
                mutation_fields.insert(name.clone(), field_sdl);
            } else {
                query_fields.insert(name.clone(), field_sdl);
            }
            fields.insert(name, mapping);
        }
    }

    let types = emit_types(spec, &mut usage);

    let mut sdl = String::new();
    if usage.needs_json {
        sdl.push_str("scalar JSON\n\n");
    }
    sdl.push_str(&types);
    if !query_fields.is_empty() {
        let _ = write!(
            sdl,
            "type Query {{\n{}\n}}\n\n",
            query_fields.values().cloned().collect::<Vec<_>>().join("\n")
        );
    }
    if !mutation_fields.is_empty() {
        let _ = write!(
            sdl,
            "type Mutation {{\n{}\n}}\n\n",
            mutation_fields.values().cloned().collect::<Vec<_>>().join("\n")
        );
    }

    Ok(GeneratedSubgraph {
        sdl: sdl.trim_end().to_string(),
        connector: RestConnectorConfig {
            base_url,
            headers: BTreeMap::new().into_iter().collect(),
            fields: fields.into_iter().collect(),
            entities: BTreeMap::new().into_iter().collect(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_name_keeps_valid_identifiers() {
        assert_eq!(sanitize_name("getPetById"), "getPetById");
        assert_eq!(sanitize_name("Pet_2"), "Pet_2");
    }

    #[test]
    fn sanitize_name_replaces_invalid_chars() {
        assert_eq!(sanitize_name("get-pet.by/id"), "get_pet_by_id");
    }

    #[test]
    fn sanitize_name_prefixes_leading_digit() {
        assert_eq!(sanitize_name("2fast"), "_2fast");
    }

    #[test]
    fn scalar_for_maps_primitives() {
        assert_eq!(scalar_for("integer", None), "Int");
        assert_eq!(scalar_for("integer", Some("int64")), "Float");
        assert_eq!(scalar_for("number", None), "Float");
        assert_eq!(scalar_for("boolean", None), "Boolean");
        assert_eq!(scalar_for("string", None), "String");
        assert_eq!(scalar_for("string", Some("date-time")), "String");
    }

    #[test]
    fn generate_rejects_non_object() {
        let spec = serde_json::json!([1, 2, 3]);
        assert_eq!(generate(&spec), Err(OpenApiError::NotAnObject));
    }

    #[test]
    fn generate_rejects_missing_paths() {
        let spec = serde_json::json!({"openapi": "3.0.0"});
        assert_eq!(generate(&spec), Err(OpenApiError::NoPaths));
    }

    fn petstore() -> Value {
        serde_json::json!({
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com/v1"}],
            "paths": {
                "/pets/{petId}": {
                    "get": {
                        "operationId": "getPetById",
                        "parameters": [
                            {"name": "petId", "in": "path", "required": true,
                             "schema": {"type": "integer"}}
                        ],
                        "responses": {
                            "200": {"content": {"application/json": {
                                "schema": {"$ref": "#/components/schemas/Pet"}}}}
                        }
                    }
                },
                "/pets": {
                    "get": {
                        "operationId": "listPets",
                        "parameters": [
                            {"name": "status", "in": "query",
                             "schema": {"type": "string"}}
                        ],
                        "responses": {
                            "200": {"content": {"application/json": {
                                "schema": {"type": "array",
                                           "items": {"$ref": "#/components/schemas/Pet"}}}}}
                        }
                    },
                    "post": {
                        "operationId": "createPet",
                        "requestBody": {
                            "required": true,
                            "content": {"application/json": {
                                "schema": {"$ref": "#/components/schemas/Pet"}}}
                        },
                        "responses": {
                            "201": {"content": {"application/json": {
                                "schema": {"$ref": "#/components/schemas/Pet"}}}}
                        }
                    }
                }
            },
            "components": {"schemas": {"Pet": {
                "type": "object",
                "required": ["id", "name"],
                "properties": {
                    "id": {"type": "integer", "format": "int64"},
                    "name": {"type": "string"},
                    "tag": {"type": "string"}
                }
            }}}
        })
    }

    #[test]
    fn generate_reads_base_url_from_servers() {
        let g = generate(&petstore()).unwrap();
        assert_eq!(g.connector.base_url, "https://api.example.com/v1");
    }

    #[test]
    fn generate_emits_output_type_from_component() {
        let g = generate(&petstore()).unwrap();
        // id is int64 -> Float, required -> non-null; tag optional
        assert!(g.sdl.contains("type Pet {"), "sdl:\n{}", g.sdl);
        assert!(g.sdl.contains("id: Float!"), "sdl:\n{}", g.sdl);
        assert!(g.sdl.contains("name: String!"), "sdl:\n{}", g.sdl);
        assert!(g.sdl.contains("tag: String\n"), "sdl:\n{}", g.sdl);
    }

    #[test]
    fn generate_maps_get_with_path_param_to_query_field() {
        let g = generate(&petstore()).unwrap();
        assert!(g.sdl.contains("getPetById(petId: Int!): Pet"), "sdl:\n{}", g.sdl);
        let m = g.connector.fields.get("getPetById").unwrap();
        assert_eq!(m.method, "GET");
        assert_eq!(m.path, "/pets/{petId}");
        assert!(m.query.is_empty());
        assert_eq!(m.body, None);
    }

    #[test]
    fn generate_maps_query_param_and_array_return() {
        let g = generate(&petstore()).unwrap();
        assert!(g.sdl.contains("listPets(status: String): [Pet]"), "sdl:\n{}", g.sdl);
        let m = g.connector.fields.get("listPets").unwrap();
        assert_eq!(m.query.get("status"), Some(&"status".to_string()));
    }

    #[test]
    fn generate_maps_post_to_mutation_with_input_body() {
        let g = generate(&petstore()).unwrap();
        assert!(g.sdl.contains("type Mutation {"), "sdl:\n{}", g.sdl);
        assert!(g.sdl.contains("createPet(input: PetInput!): Pet"), "sdl:\n{}", g.sdl);
        assert!(g.sdl.contains("input PetInput {"), "sdl:\n{}", g.sdl);
        let m = g.connector.fields.get("createPet").unwrap();
        assert_eq!(m.method, "POST");
        assert_eq!(m.body, Some("input".to_string()));
    }

    #[test]
    fn generate_synthesizes_field_name_without_operation_id() {
        let spec = serde_json::json!({
            "paths": {"/pets/{petId}": {"get": {
                "responses": {"200": {"content": {"application/json": {
                    "schema": {"type": "string"}}}}}
            }}}
        });
        let g = generate(&spec).unwrap();
        assert!(
            g.connector.fields.contains_key("getPetsPetId"),
            "{:?}",
            g.connector.fields.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_declares_json_scalar_only_when_used() {
        let g = generate(&petstore()).unwrap();
        assert!(!g.sdl.contains("scalar JSON"), "sdl:\n{}", g.sdl);

        let spec = serde_json::json!({
            "paths": {"/raw": {"get": {"operationId": "raw", "responses": {
                "200": {"content": {"application/json": {
                    "schema": {"type": "object"}}}}}}}}
        });
        let g2 = generate(&spec).unwrap();
        assert!(g2.sdl.contains("scalar JSON"), "sdl:\n{}", g2.sdl);
        assert!(g2.sdl.contains("raw: JSON"), "sdl:\n{}", g2.sdl);
    }
}
