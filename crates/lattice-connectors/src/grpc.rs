//! gRPC ingestion: turn a compiled protobuf descriptor set into a federated
//! gRPC subgraph.
//!
//! [`generate`] reads a `FileDescriptorSet` (produced by `protoc
//! --descriptor_set_out`) and produces two artifacts: a GraphQL SDL (for
//! composition into the supergraph) and a [`GrpcConnectorConfig`] (the per-field
//! method mappings the gRPC executor runs at request time). Each unary method on
//! a service becomes a root field, and each referenced message becomes a GraphQL
//! type. Streaming methods are skipped (not representable as a single field).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use lattice_config::{GrpcConnectorConfig, GrpcMethodMapping};
use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, EnumDescriptorProto, FileDescriptorSet, MethodDescriptorProto};
use thiserror::Error;

/// Errors raised while generating a subgraph from a protobuf descriptor set.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GrpcError {
    /// The descriptor set had no service with a unary method to expose.
    #[error("descriptor set has no service with a unary method")]
    NoUnaryMethods,
}

/// A subgraph generated from a protobuf descriptor set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedGrpcSubgraph {
    /// GraphQL SDL describing the generated types and root fields.
    pub sdl: String,
    /// gRPC connector config (endpoint, descriptor path, per-field mappings).
    pub connector: GrpcConnectorConfig,
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

/// Lower-camel-case a proto identifier, accepting both `snake_case` and
/// `PascalCase` input (`GetOrder` -> `getOrder`, `order_id` -> `orderId`).
fn lower_camel(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, part) in raw.split('_').filter(|p| !p.is_empty()).enumerate() {
        let mut chars = part.chars();
        let Some(first) = chars.next() else { continue };
        if i == 0 {
            out.extend(first.to_lowercase());
        } else {
            out.extend(first.to_uppercase());
        }
        out.push_str(chars.as_str());
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// A message or enum resolved from the descriptor set, keyed by its protobuf
/// fully-qualified name (leading-dot form, e.g. `.orders.v1.Order`).
struct Registry<'a> {
    messages: BTreeMap<String, &'a DescriptorProto>,
    enums: BTreeMap<String, &'a EnumDescriptorProto>,
    /// Protobuf FQN to the GraphQL base type name (nested types are flattened
    /// with `_`, e.g. `.orders.v1.Order.Status` -> `Order_Status`).
    gql_names: BTreeMap<String, String>,
}

impl<'a> Registry<'a> {
    fn build(fds: &'a FileDescriptorSet) -> Self {
        let mut reg = Self {
            messages: BTreeMap::new(),
            enums: BTreeMap::new(),
            gql_names: BTreeMap::new(),
        };
        for file in &fds.file {
            let pkg = file.package();
            // The dotted FQN prefix for top-level types in this file's package.
            let base_fqn = if pkg.is_empty() {
                String::new()
            } else {
                format!(".{pkg}")
            };
            for m in &file.message_type {
                reg.walk_message(&base_fqn, "", m);
            }
            for e in &file.enum_type {
                reg.insert_enum(&base_fqn, "", e);
            }
        }
        reg
    }

    fn walk_message(&mut self, outer_fqn: &str, outer_gql: &str, m: &'a DescriptorProto) {
        let simple = m.name();
        let fqn = format!("{outer_fqn}.{simple}");
        let gql = gql_join(outer_gql, simple);
        self.messages.insert(fqn.clone(), m);
        self.gql_names.insert(fqn.clone(), sanitize_name(&gql));
        for nested in &m.nested_type {
            self.walk_message(&fqn, &gql, nested);
        }
        for e in &m.enum_type {
            self.insert_enum(&fqn, &gql, e);
        }
    }

    fn insert_enum(&mut self, outer_fqn: &str, outer_gql: &str, e: &'a EnumDescriptorProto) {
        let simple = e.name();
        let fqn = format!("{outer_fqn}.{simple}");
        let gql = gql_join(outer_gql, simple);
        self.enums.insert(fqn.clone(), e);
        self.gql_names.insert(fqn, sanitize_name(&gql));
    }
}

/// Join a parent GraphQL name and a child name with `_` (flattening nesting).
fn gql_join(outer_gql: &str, name: &str) -> String {
    if outer_gql.is_empty() {
        name.to_string()
    } else {
        format!("{outer_gql}_{name}")
    }
}

/// Whether `method` is a unary call (no client or server streaming).
fn is_unary(method: &MethodDescriptorProto) -> bool {
    !method.client_streaming() && !method.server_streaming()
}

/// Whether a unary method maps to a `Query` (read) rather than a `Mutation`.
///
/// Prefers the proto `idempotency_level` option; falls back to a name-prefix
/// heuristic when it is unset, mirroring the OpenAPI `GET` -> `Query` rule.
fn is_query(method: &MethodDescriptorProto) -> bool {
    use prost_types::method_options::IdempotencyLevel;
    const READ_PREFIXES: [&str; 7] = ["Get", "List", "Search", "Lookup", "Fetch", "Read", "Query"];
    if let Some(opts) = &method.options {
        match opts.idempotency_level() {
            IdempotencyLevel::NoSideEffects => return true,
            IdempotencyLevel::Idempotent | IdempotencyLevel::IdempotencyUnknown => {}
        }
    }
    let name = method.name();
    READ_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Accumulated set of types to emit, discovered by transitive closure.
#[derive(Default)]
struct Usage {
    output_msgs: BTreeSet<String>,
    input_msgs: BTreeSet<String>,
    enums: BTreeSet<String>,
    needs_json: bool,
}

/// Whether a message descriptor is a synthetic protobuf `map<k,v>` entry.
fn is_map_entry(m: &DescriptorProto) -> bool {
    m.options.as_ref().and_then(|o| o.map_entry).unwrap_or(false)
}

/// The GraphQL base type name for a field, recording any referenced type in
/// `usage` so it is emitted. Map and group fields degrade to the `JSON` scalar.
fn base_type(field: &prost_types::FieldDescriptorProto, reg: &Registry, usage: &mut Usage, input: bool) -> String {
    match field.r#type() {
        Type::Double | Type::Float => "Float".to_string(),
        Type::Int32 | Type::Sint32 | Type::Sfixed32 | Type::Uint32 | Type::Fixed32 => "Int".to_string(),
        Type::Bool => "Boolean".to_string(),
        // String and bytes are strings in proto3 JSON (bytes base64-encoded); so
        // are 64-bit integers, which proto3 JSON renders as decimal strings.
        Type::String | Type::Bytes | Type::Int64 | Type::Uint64 | Type::Sint64 | Type::Fixed64 | Type::Sfixed64 => {
            "String".to_string()
        }
        Type::Enum => {
            let tn = field.type_name();
            reg.gql_names.get(tn).map_or_else(
                || {
                    usage.needs_json = true;
                    "JSON".to_string()
                },
                |name| {
                    usage.enums.insert(tn.to_string());
                    name.clone()
                },
            )
        }
        Type::Message | Type::Group => {
            let tn = field.type_name();
            if reg.messages.get(tn).is_some_and(|m| is_map_entry(m)) {
                usage.needs_json = true;
                return "JSON".to_string();
            }
            if let Some(name) = reg.gql_names.get(tn) {
                if input {
                    usage.input_msgs.insert(tn.to_string());
                    format!("{name}Input")
                } else {
                    usage.output_msgs.insert(tn.to_string());
                    name.clone()
                }
            } else {
                usage.needs_json = true;
                "JSON".to_string()
            }
        }
    }
}

/// Render a field's full GraphQL type (list wrapping and nullability). Singular
/// fields are nullable; repeated fields become a list of non-null elements.
fn field_type(field: &prost_types::FieldDescriptorProto, reg: &Registry, usage: &mut Usage, input: bool) -> String {
    let base = base_type(field, reg, usage, input);
    if field.label() == Label::Repeated {
        format!("[{base}!]")
    } else {
        base
    }
}

/// The GraphQL field name for a proto field (its `json_name`, or camel-cased).
fn field_name(field: &prost_types::FieldDescriptorProto) -> String {
    let jn = field.json_name();
    if jn.is_empty() {
        lower_camel(field.name())
    } else {
        sanitize_name(jn)
    }
}

/// Generate a GraphQL SDL and gRPC connector config from a descriptor set.
///
/// `endpoint` is the gRPC server address and `descriptor_set` the path the
/// executor loads for wire coding; neither is carried in the descriptor itself.
///
/// # Errors
///
/// Returns [`GrpcError::NoUnaryMethods`] if no service exposes a unary method.
pub fn generate(
    fds: &FileDescriptorSet,
    endpoint: &str,
    descriptor_set: &str,
) -> Result<GeneratedGrpcSubgraph, GrpcError> {
    let reg = Registry::build(fds);
    let mut usage = Usage::default();

    // Collect root fields (one per unary method), split into queries/mutations.
    let mut queries: BTreeMap<String, String> = BTreeMap::new();
    let mut mutations: BTreeMap<String, String> = BTreeMap::new();
    let mut fields: BTreeMap<String, GrpcMethodMapping> = BTreeMap::new();

    for file in &fds.file {
        let pkg = file.package();
        for service in &file.service {
            let service_fqn = if pkg.is_empty() {
                service.name().to_string()
            } else {
                format!("{pkg}.{}", service.name())
            };
            for method in &service.method {
                if !is_unary(method) {
                    continue;
                }
                let gql_field = lower_camel(method.name());
                let input_name = reg.gql_names.get(method.input_type()).cloned();
                let output_name = reg.gql_names.get(method.output_type()).cloned();

                // Request message becomes an input type; response an output type.
                let arg = if let Some(name) = &input_name {
                    usage.input_msgs.insert(method.input_type().to_string());
                    format!("(input: {name}Input!)")
                } else {
                    usage.needs_json = true;
                    "(input: JSON)".to_string()
                };
                let result = if let Some(name) = &output_name {
                    usage.output_msgs.insert(method.output_type().to_string());
                    name.clone()
                } else {
                    usage.needs_json = true;
                    "JSON".to_string()
                };

                let line = format!("{gql_field}{arg}: {result}");
                if is_query(method) {
                    queries.insert(gql_field.clone(), line);
                } else {
                    mutations.insert(gql_field.clone(), line);
                }
                fields.insert(
                    gql_field,
                    GrpcMethodMapping {
                        service: service_fqn.clone(),
                        method: method.name().to_string(),
                        input: "input".to_string(),
                    },
                );
            }
        }
    }

    if fields.is_empty() {
        return Err(GrpcError::NoUnaryMethods);
    }

    // Transitive closure over message fields: output messages pull in further
    // output messages, input messages pull in further input messages.
    close_output(&reg, &mut usage);
    close_input(&reg, &mut usage);

    let sdl = render_sdl(&reg, &usage, &queries, &mutations);
    let connector = GrpcConnectorConfig {
        endpoint: endpoint.to_string(),
        descriptor_set: descriptor_set.to_string(),
        metadata: std::collections::HashMap::new(),
        fields: fields.into_iter().collect(),
    };
    Ok(GeneratedGrpcSubgraph { sdl, connector })
}

/// Expand `usage.output_msgs` until no new output message is referenced.
fn close_output(reg: &Registry, usage: &mut Usage) {
    let mut done: BTreeSet<String> = BTreeSet::new();
    while let Some(fqn) = usage.output_msgs.iter().find(|f| !done.contains(*f)).cloned() {
        done.insert(fqn.clone());
        if let Some(m) = reg.messages.get(&fqn) {
            for field in &m.field {
                let _ = base_type(field, reg, usage, false);
            }
        }
    }
}

/// Expand `usage.input_msgs` until no new input message is referenced.
fn close_input(reg: &Registry, usage: &mut Usage) {
    let mut done: BTreeSet<String> = BTreeSet::new();
    while let Some(fqn) = usage.input_msgs.iter().find(|f| !done.contains(*f)).cloned() {
        done.insert(fqn.clone());
        if let Some(m) = reg.messages.get(&fqn) {
            for field in &m.field {
                let _ = base_type(field, reg, usage, true);
            }
        }
    }
}

/// Render the full SDL: JSON scalar, enums, output types, input types, and the
/// `Query`/`Mutation` root types, each block sorted for deterministic output.
fn render_sdl(
    reg: &Registry,
    usage: &Usage,
    queries: &BTreeMap<String, String>,
    mutations: &BTreeMap<String, String>,
) -> String {
    let mut sdl = String::new();

    if usage.needs_json {
        sdl.push_str("scalar JSON\n\n");
    }

    for fqn in &usage.enums {
        if let Some(e) = reg.enums.get(fqn) {
            let name = &reg.gql_names[fqn];
            let _ = writeln!(sdl, "enum {name} {{");
            for value in &e.value {
                let _ = writeln!(sdl, "  {}", sanitize_name(value.name()));
            }
            sdl.push_str("}\n\n");
        }
    }

    for fqn in &usage.output_msgs {
        if let Some(m) = reg.messages.get(fqn) {
            let name = &reg.gql_names[fqn];
            let _ = writeln!(sdl, "type {name} {{");
            render_fields(reg, m, false, &mut sdl);
            sdl.push_str("}\n\n");
        }
    }

    for fqn in &usage.input_msgs {
        if let Some(m) = reg.messages.get(fqn) {
            let name = &reg.gql_names[fqn];
            let _ = writeln!(sdl, "input {name}Input {{");
            render_fields(reg, m, true, &mut sdl);
            sdl.push_str("}\n\n");
        }
    }

    if !queries.is_empty() {
        sdl.push_str("type Query {\n");
        for line in queries.values() {
            let _ = writeln!(sdl, "  {line}");
        }
        sdl.push_str("}\n\n");
    }

    if !mutations.is_empty() {
        sdl.push_str("type Mutation {\n");
        for line in mutations.values() {
            let _ = writeln!(sdl, "  {line}");
        }
        sdl.push_str("}\n\n");
    }

    sdl.truncate(sdl.trim_end().len());
    sdl.push('\n');
    sdl
}

/// Render a message's fields as GraphQL field lines into `sdl`. A throwaway
/// `Usage` absorbs the closure side effects already computed by the caller.
fn render_fields(reg: &Registry, m: &DescriptorProto, input: bool, sdl: &mut String) {
    let mut scratch = Usage::default();
    let mut lines: BTreeMap<String, String> = BTreeMap::new();
    for field in &m.field {
        let ty = field_type(field, reg, &mut scratch, input);
        lines.insert(field_name(field), ty);
    }
    for (name, ty) in lines {
        let _ = writeln!(sdl, "  {name}: {ty}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{FieldDescriptorProto, FileDescriptorProto, MethodOptions, ServiceDescriptorProto};

    fn scalar(name: &str, number: i32, ty: Type, label: Label) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.to_string()),
            number: Some(number),
            label: Some(label as i32),
            r#type: Some(ty as i32),
            ..Default::default()
        }
    }

    fn message_field(name: &str, number: i32, type_name: &str, label: Label) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.to_string()),
            number: Some(number),
            label: Some(label as i32),
            r#type: Some(Type::Message as i32),
            type_name: Some(type_name.to_string()),
            ..Default::default()
        }
    }

    fn message(name: &str, fields: Vec<FieldDescriptorProto>) -> DescriptorProto {
        DescriptorProto {
            name: Some(name.to_string()),
            field: fields,
            ..Default::default()
        }
    }

    fn method(name: &str, input: &str, output: &str) -> MethodDescriptorProto {
        MethodDescriptorProto {
            name: Some(name.to_string()),
            input_type: Some(input.to_string()),
            output_type: Some(output.to_string()),
            client_streaming: Some(false),
            server_streaming: Some(false),
            ..Default::default()
        }
    }

    /// Build a single-file descriptor set in package `orders.v1` with the given
    /// messages, enums, and one service `OrderService` of the given methods.
    fn descriptor_set(
        messages: Vec<DescriptorProto>,
        enums: Vec<EnumDescriptorProto>,
        methods: Vec<MethodDescriptorProto>,
    ) -> FileDescriptorSet {
        FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("orders.proto".to_string()),
                package: Some("orders.v1".to_string()),
                message_type: messages,
                enum_type: enums,
                service: vec![ServiceDescriptorProto {
                    name: Some("OrderService".to_string()),
                    method: methods,
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn generates_query_field_and_types_for_a_unary_method() {
        let fds = descriptor_set(
            vec![
                message("GetOrderRequest", vec![scalar("id", 1, Type::String, Label::Optional)]),
                message(
                    "Order",
                    vec![
                        scalar("id", 1, Type::String, Label::Optional),
                        scalar("total", 2, Type::Double, Label::Optional),
                    ],
                ),
            ],
            vec![],
            vec![{
                let mut m = method("GetOrder", ".orders.v1.GetOrderRequest", ".orders.v1.Order");
                m.options = Some(MethodOptions {
                    idempotency_level: Some(prost_types::method_options::IdempotencyLevel::NoSideEffects as i32),
                    ..Default::default()
                });
                m
            }],
        );

        let out = generate(&fds, "http://orders.internal:50051", "./orders.pb").unwrap();

        // The response message is an output type; the request is an input type.
        assert!(out.sdl.contains("type Order {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("id: String"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("total: Float"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("input GetOrderRequestInput {"), "sdl:\n{}", out.sdl);
        // A no-side-effects method becomes a Query field.
        assert!(
            out.sdl.contains("getOrder(input: GetOrderRequestInput!): Order"),
            "sdl:\n{}",
            out.sdl
        );

        // The connector carries endpoint, descriptor path, and the mapping.
        assert_eq!(out.connector.endpoint, "http://orders.internal:50051");
        assert_eq!(out.connector.descriptor_set, "./orders.pb");
        let mapping = out.connector.fields.get("getOrder").unwrap();
        assert_eq!(mapping.service, "orders.v1.OrderService");
        assert_eq!(mapping.method, "GetOrder");
        assert_eq!(mapping.input, "input");
    }

    #[test]
    fn non_idempotent_method_becomes_a_mutation() {
        let fds = descriptor_set(
            vec![
                message(
                    "CreateOrderRequest",
                    vec![scalar("name", 1, Type::String, Label::Optional)],
                ),
                message("Order", vec![scalar("id", 1, Type::String, Label::Optional)]),
            ],
            vec![],
            // No idempotency option, name does not match a read prefix -> Mutation.
            vec![method(
                "CreateOrder",
                ".orders.v1.CreateOrderRequest",
                ".orders.v1.Order",
            )],
        );

        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("type Mutation {"), "sdl:\n{}", out.sdl);
        assert!(
            out.sdl.contains("createOrder(input: CreateOrderRequestInput!): Order"),
            "sdl:\n{}",
            out.sdl
        );
        assert!(!out.sdl.contains("type Query {"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn read_prefix_without_option_becomes_a_query() {
        let fds = descriptor_set(
            vec![
                message("ListOrdersRequest", vec![]),
                message("Order", vec![scalar("id", 1, Type::String, Label::Optional)]),
            ],
            vec![],
            vec![method("ListOrders", ".orders.v1.ListOrdersRequest", ".orders.v1.Order")],
        );
        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("type Query {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("listOrders(input: ListOrdersRequestInput!): Order"));
    }

    #[test]
    fn repeated_field_becomes_a_list() {
        let fds = descriptor_set(
            vec![
                message("ListReq", vec![]),
                message("Order", vec![scalar("tags", 1, Type::String, Label::Repeated)]),
            ],
            vec![],
            vec![method("ListOrders", ".orders.v1.ListReq", ".orders.v1.Order")],
        );
        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("tags: [String!]"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn scalar_widths_and_bytes_map_to_graphql() {
        let fds = descriptor_set(
            vec![
                message("Req", vec![]),
                message(
                    "Wide",
                    vec![
                        scalar("count", 1, Type::Int32, Label::Optional),
                        scalar("big", 2, Type::Int64, Label::Optional),
                        scalar("ok", 3, Type::Bool, Label::Optional),
                        scalar("blob", 4, Type::Bytes, Label::Optional),
                    ],
                ),
            ],
            vec![],
            vec![method("GetWide", ".orders.v1.Req", ".orders.v1.Wide")],
        );
        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("count: Int"), "sdl:\n{}", out.sdl);
        // 64-bit integers are strings in the proto3 JSON mapping
        assert!(out.sdl.contains("big: String"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("ok: Boolean"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("blob: String"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn nested_message_and_enum_are_emitted() {
        let status_enum = EnumDescriptorProto {
            name: Some("Status".to_string()),
            value: vec![
                prost_types::EnumValueDescriptorProto {
                    name: Some("PENDING".to_string()),
                    number: Some(0),
                    ..Default::default()
                },
                prost_types::EnumValueDescriptorProto {
                    name: Some("SHIPPED".to_string()),
                    number: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let fds = descriptor_set(
            vec![
                message("Req", vec![]),
                message(
                    "Order",
                    vec![
                        message_field("customer", 1, ".orders.v1.Customer", Label::Optional),
                        FieldDescriptorProto {
                            name: Some("status".to_string()),
                            number: Some(2),
                            label: Some(Label::Optional as i32),
                            r#type: Some(Type::Enum as i32),
                            type_name: Some(".orders.v1.Status".to_string()),
                            ..Default::default()
                        },
                    ],
                ),
                message("Customer", vec![scalar("name", 1, Type::String, Label::Optional)]),
            ],
            vec![status_enum],
            vec![method("GetOrder", ".orders.v1.Req", ".orders.v1.Order")],
        );
        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("type Customer {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("customer: Customer"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("enum Status {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("PENDING"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("status: Status"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn map_field_degrades_to_json_scalar() {
        let map_entry = DescriptorProto {
            name: Some("LabelsEntry".to_string()),
            field: vec![
                scalar("key", 1, Type::String, Label::Optional),
                scalar("value", 2, Type::String, Label::Optional),
            ],
            options: Some(prost_types::MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let order = DescriptorProto {
            name: Some("Order".to_string()),
            field: vec![message_field(
                "labels",
                1,
                ".orders.v1.Order.LabelsEntry",
                Label::Repeated,
            )],
            nested_type: vec![map_entry],
            ..Default::default()
        };
        let fds = descriptor_set(
            vec![message("Req", vec![]), order],
            vec![],
            vec![method("GetOrder", ".orders.v1.Req", ".orders.v1.Order")],
        );
        let out = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert!(out.sdl.contains("scalar JSON"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("labels: [JSON!]"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn streaming_only_service_yields_no_unary_methods() {
        let mut streaming = method("Watch", ".orders.v1.Req", ".orders.v1.Order");
        streaming.server_streaming = Some(true);
        let fds = descriptor_set(
            vec![message("Req", vec![]), message("Order", vec![])],
            vec![],
            vec![streaming],
        );
        assert_eq!(generate(&fds, "http://x:1", "./x.pb"), Err(GrpcError::NoUnaryMethods));
    }

    #[test]
    fn output_is_deterministic() {
        let fds = descriptor_set(
            vec![
                message("Req", vec![scalar("id", 1, Type::String, Label::Optional)]),
                message("Order", vec![scalar("id", 1, Type::String, Label::Optional)]),
            ],
            vec![],
            vec![method("GetOrder", ".orders.v1.Req", ".orders.v1.Order")],
        );
        let a = generate(&fds, "http://x:1", "./x.pb").unwrap();
        let b = generate(&fds, "http://x:1", "./x.pb").unwrap();
        assert_eq!(a, b);
    }
}
