//! OData ingestion: turn an OData 4.0 `$metadata` (EDMX) document into a
//! federated REST subgraph.
//!
//! [`generate`] reads an EDMX document and produces a GraphQL SDL plus a
//! [`RestConnectorConfig`], because an OData service is plain HTTP+JSON: the
//! existing [`crate::rest`] executor runs the generated mappings unchanged. Each
//! entity set becomes a collection query and a by-key query, and each entity or
//! complex type becomes a GraphQL type.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use lattice_config::{RestConnectorConfig, RestFieldMapping};
use roxmltree::Document;
use thiserror::Error;

/// Errors raised while generating a subgraph from an EDMX document.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ODataError {
    /// The EDMX document was not well-formed XML.
    #[error("failed to parse EDMX document: {0}")]
    InvalidXml(String),
    /// The document declared no entity sets, so there is nothing to expose.
    #[error("EDMX has no entity sets")]
    NoEntitySets,
}

/// A subgraph generated from an OData `$metadata` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSubgraph {
    /// GraphQL SDL describing the generated types and root fields.
    pub sdl: String,
    /// REST connector config (base URL and per-field mappings) for the executor.
    pub connector: RestConnectorConfig,
}

/// Convert a raw identifier into a valid GraphQL name.
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

/// Lower-case the first character (`Products` -> `products`).
fn lower_first(raw: &str) -> String {
    let mut chars = raw.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_lowercase().chain(chars).collect())
}

/// Map an `Edm.*` primitive to a GraphQL scalar.
fn edm_scalar(edm: &str) -> &'static str {
    match edm {
        "Boolean" => "Boolean",
        "Byte" | "SByte" | "Int16" | "Int32" => "Int",
        // 64-bit and fractional numbers widen to Float (32-bit Int would overflow)
        "Int64" | "Double" | "Single" | "Decimal" => "Float",
        // everything textual or temporal is a String in the OData JSON format
        _ => "String",
    }
}

/// Whether an OData key property type serializes as a quoted string in a URL
/// key segment (`/Set('abc')` versus `/Set(1)`).
fn is_string_key(edm_type: &str) -> bool {
    edm_type.strip_prefix("Edm.").is_none_or(|edm| {
        !matches!(
            edm,
            "Boolean" | "Byte" | "SByte" | "Int16" | "Int32" | "Int64" | "Double" | "Single" | "Decimal"
        )
    })
}

/// The GraphQL base type name for an OData property type (unwrapping a
/// `Collection(...)` and resolving a `namespace.Type` reference to its last
/// segment). Returns the base name and whether it was a collection.
fn base_type(odata_type: &str) -> (String, bool) {
    if let Some(inner) = odata_type.strip_prefix("Collection(").and_then(|s| s.strip_suffix(')')) {
        let (base, _) = base_type(inner);
        return (base, true);
    }
    odata_type.strip_prefix("Edm.").map_or_else(
        || {
            (
                sanitize_name(odata_type.rsplit('.').next().unwrap_or(odata_type)),
                false,
            )
        },
        |edm| (edm_scalar(edm).to_string(), false),
    )
}

/// Render a property's full GraphQL type (list wrapping and nullability).
fn graphql_type(odata_type: &str, nullable: bool) -> String {
    let (base, is_list) = base_type(odata_type);
    let inner = if nullable { base } else { format!("{base}!") };
    if is_list { format!("[{inner}]") } else { inner }
}

/// A key property: its name, GraphQL scalar, and whether it quotes in a URL.
struct Key {
    name: String,
    scalar: &'static str,
    quoted: bool,
}

/// Build the by-key path segment, for example `/Products({ID})` or
/// `/Orders(OrderID={OrderID},LineID={LineID})`, with string keys quoted.
fn key_path(set: &str, keys: &[Key]) -> String {
    if let [key] = keys {
        return if key.quoted {
            format!("/{set}('{{{}}}')", key.name)
        } else {
            format!("/{set}({{{}}})", key.name)
        };
    }
    let parts: Vec<String> = keys
        .iter()
        .map(|k| {
            if k.quoted {
                format!("{}='{{{}}}'", k.name, k.name)
            } else {
                format!("{}={{{}}}", k.name, k.name)
            }
        })
        .collect();
    format!("/{set}({})", parts.join(","))
}

/// A map of GraphQL type name to its fields (field name -> GraphQL type).
type TypeFields = BTreeMap<String, BTreeMap<String, String>>;

/// Collect enum definitions as a map of name to member names, document order.
fn collect_enums(doc: &Document) -> BTreeMap<String, Vec<String>> {
    let mut enums: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for en in doc.descendants().filter(|n| n.has_tag_name("EnumType")) {
        let name = sanitize_name(en.attribute("Name").unwrap_or_default());
        let members = en
            .children()
            .filter(|n| n.has_tag_name("Member"))
            .filter_map(|m| m.attribute("Name"))
            .map(sanitize_name)
            .collect();
        enums.insert(name, members);
    }
    enums
}

/// Collect structural types (entity + complex) and entity-type keys. Navigation
/// properties are skipped; they require `$expand` to resolve.
fn collect_types(doc: &Document) -> (TypeFields, BTreeMap<String, Vec<Key>>) {
    let mut types: TypeFields = BTreeMap::new();
    let mut entity_keys: BTreeMap<String, Vec<Key>> = BTreeMap::new();
    for st in doc
        .descendants()
        .filter(|n| n.has_tag_name("EntityType") || n.has_tag_name("ComplexType"))
    {
        let tname = sanitize_name(st.attribute("Name").unwrap_or_default());
        let mut fields = BTreeMap::new();
        for p in st.children().filter(|n| n.has_tag_name("Property")) {
            let pname = sanitize_name(p.attribute("Name").unwrap_or_default());
            let ptype = p.attribute("Type").unwrap_or("Edm.String");
            let nullable = p.attribute("Nullable").is_none_or(|v| v != "false");
            fields.insert(pname, graphql_type(ptype, nullable));
        }

        if st.has_tag_name("EntityType") {
            entity_keys.insert(tname.clone(), collect_keys(st));
        }
        types.insert(tname, fields);
    }
    (types, entity_keys)
}

/// Collect an entity type's key properties, resolving each to its scalar type.
fn collect_keys(entity: roxmltree::Node) -> Vec<Key> {
    let prop_types: BTreeMap<&str, &str> = entity
        .children()
        .filter(|n| n.has_tag_name("Property"))
        .filter_map(|p| Some((p.attribute("Name")?, p.attribute("Type")?)))
        .collect();
    entity
        .children()
        .find(|n| n.has_tag_name("Key"))
        .into_iter()
        .flat_map(|k| k.children().filter(|n| n.has_tag_name("PropertyRef")))
        .filter_map(|r| r.attribute("Name"))
        .map(|kn| {
            let ktype = prop_types.get(kn).copied().unwrap_or("Edm.String");
            Key {
                name: sanitize_name(kn),
                scalar: edm_scalar(ktype.strip_prefix("Edm.").unwrap_or("String")),
                quoted: is_string_key(ktype),
            }
        })
        .collect()
}

/// Generate a GraphQL SDL and REST connector config from an EDMX document.
///
/// `base_url` is the OData service root (for example
/// `https://services.odata.org/V4/Northwind/Northwind.svc`); entity-set paths
/// are appended to it.
///
/// # Errors
///
/// Returns [`ODataError::InvalidXml`] if the document is not well-formed, or
/// [`ODataError::NoEntitySets`] if it declares no entity sets.
pub fn generate(edmx: &str, base_url: &str) -> Result<GeneratedSubgraph, ODataError> {
    let doc = Document::parse(edmx).map_err(|e| ODataError::InvalidXml(e.to_string()))?;

    let enums = collect_enums(&doc);
    let (types, entity_keys) = collect_types(&doc);

    // Entity sets -> a collection query and (if keyed) a by-key query.
    let mut queries: BTreeMap<String, String> = BTreeMap::new();
    let mut fields: BTreeMap<String, RestFieldMapping> = BTreeMap::new();
    for es in doc.descendants().filter(|n| n.has_tag_name("EntitySet")) {
        let set_name = es.attribute("Name").unwrap_or_default();
        let et_ref = es.attribute("EntityType").unwrap_or_default();
        let type_name = sanitize_name(et_ref.rsplit('.').next().unwrap_or(et_ref));

        // Collection query.
        let coll_field = lower_first(&sanitize_name(set_name));
        queries.insert(coll_field.clone(), format!("{coll_field}: [{type_name}!]"));
        fields.insert(
            coll_field,
            RestFieldMapping {
                method: "GET".to_string(),
                path: format!("/{set_name}"),
                query: std::collections::HashMap::new(),
                select: Some("/value".to_string()),
                body: None,
                response: std::collections::HashMap::new(),
            },
        );

        // By-key query, when the entity type has a key.
        if let Some(keys) = entity_keys.get(&type_name).filter(|k| !k.is_empty()) {
            let by_key_field = lower_first(&type_name);
            let args: Vec<String> = keys.iter().map(|k| format!("{}: {}!", k.name, k.scalar)).collect();
            queries.insert(
                by_key_field.clone(),
                format!("{by_key_field}({}): {type_name}", args.join(", ")),
            );
            fields.insert(
                by_key_field,
                RestFieldMapping {
                    method: "GET".to_string(),
                    path: key_path(set_name, keys),
                    query: std::collections::HashMap::new(),
                    select: None,
                    body: None,
                    response: std::collections::HashMap::new(),
                },
            );
        }
    }

    if fields.is_empty() {
        return Err(ODataError::NoEntitySets);
    }

    let sdl = render_sdl(&enums, &types, &queries);
    let connector = RestConnectorConfig {
        base_url: base_url.to_string(),
        headers: std::collections::HashMap::new(),
        fields: fields.into_iter().collect(),
        entities: std::collections::HashMap::new(),
    };
    Ok(GeneratedSubgraph { sdl, connector })
}

/// Render the SDL: enums, object types, and the `Query` root, sorted for
/// deterministic output.
fn render_sdl(
    enums: &BTreeMap<String, Vec<String>>,
    types: &BTreeMap<String, BTreeMap<String, String>>,
    queries: &BTreeMap<String, String>,
) -> String {
    let mut sdl = String::new();

    for (name, members) in enums {
        let _ = writeln!(sdl, "enum {name} {{");
        for member in members {
            let _ = writeln!(sdl, "  {member}");
        }
        sdl.push_str("}\n\n");
    }

    for (name, props) in types {
        let _ = writeln!(sdl, "type {name} {{");
        for (field, ty) in props {
            let _ = writeln!(sdl, "  {field}: {ty}");
        }
        sdl.push_str("}\n\n");
    }

    if !queries.is_empty() {
        sdl.push_str("type Query {\n");
        for line in queries.values() {
            let _ = writeln!(sdl, "  {line}");
        }
        sdl.push_str("}\n\n");
    }

    sdl.truncate(sdl.trim_end().len());
    sdl.push('\n');
    sdl
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORTHWIND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
  <edmx:DataServices>
    <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="NW">
      <EnumType Name="Status">
        <Member Name="Active" Value="0"/>
        <Member Name="Discontinued" Value="1"/>
      </EnumType>
      <EntityType Name="Product">
        <Key><PropertyRef Name="ID"/></Key>
        <Property Name="ID" Type="Edm.Int32" Nullable="false"/>
        <Property Name="Name" Type="Edm.String"/>
        <Property Name="Price" Type="Edm.Double"/>
        <Property Name="Status" Type="NW.Status"/>
        <NavigationProperty Name="Category" Type="NW.Category"/>
      </EntityType>
      <EntityContainer Name="Container">
        <EntitySet Name="Products" EntityType="NW.Product"/>
      </EntityContainer>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

    #[test]
    fn generates_collection_and_by_key_queries() {
        let out = generate(NORTHWIND, "https://svc.example.com/odata").unwrap();

        // Entity type and enum are emitted; property names match the JSON keys.
        assert!(out.sdl.contains("type Product {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("ID: Int!"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("Name: String"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("Price: Float"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("Status: Status"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("enum Status {"), "sdl:\n{}", out.sdl);
        // Navigation properties are not emitted (they need $expand).
        assert!(!out.sdl.contains("Category"), "sdl:\n{}", out.sdl);

        // Collection query returns a list; by-key query takes the key.
        assert!(out.sdl.contains("products: [Product!]"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("product(ID: Int!): Product"), "sdl:\n{}", out.sdl);

        // Connector mappings: collection selects /value; by-key hits /Products(id).
        assert_eq!(out.connector.base_url, "https://svc.example.com/odata");
        let coll = out.connector.fields.get("products").unwrap();
        assert_eq!(coll.path, "/Products");
        assert_eq!(coll.select.as_deref(), Some("/value"));
        let by_key = out.connector.fields.get("product").unwrap();
        assert_eq!(by_key.path, "/Products({ID})");
        assert_eq!(by_key.select, None);
    }

    fn schema(body: &str) -> String {
        format!(
            r#"<edmx:Edmx xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx" Version="4.0">
<edmx:DataServices><Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="NS">
{body}
</Schema></edmx:DataServices></edmx:Edmx>"#
        )
    }

    #[test]
    fn string_key_is_quoted_in_the_path() {
        let edmx = schema(
            r#"<EntityType Name="Customer">
                 <Key><PropertyRef Name="Code"/></Key>
                 <Property Name="Code" Type="Edm.String" Nullable="false"/>
               </EntityType>
               <EntityContainer Name="C"><EntitySet Name="Customers" EntityType="NS.Customer"/></EntityContainer>"#,
        );
        let out = generate(&edmx, "http://x").unwrap();
        assert_eq!(
            out.connector.fields.get("customer").unwrap().path,
            "/Customers('{Code}')"
        );
        assert!(
            out.sdl.contains("customer(Code: String!): Customer"),
            "sdl:\n{}",
            out.sdl
        );
    }

    #[test]
    fn composite_key_builds_a_named_path() {
        let edmx = schema(
            r#"<EntityType Name="Line">
                 <Key><PropertyRef Name="OrderID"/><PropertyRef Name="SKU"/></Key>
                 <Property Name="OrderID" Type="Edm.Int32" Nullable="false"/>
                 <Property Name="SKU" Type="Edm.String" Nullable="false"/>
               </EntityType>
               <EntityContainer Name="C"><EntitySet Name="Lines" EntityType="NS.Line"/></EntityContainer>"#,
        );
        let out = generate(&edmx, "http://x").unwrap();
        assert_eq!(
            out.connector.fields.get("line").unwrap().path,
            "/Lines(OrderID={OrderID},SKU='{SKU}')"
        );
    }

    #[test]
    fn complex_type_and_collection_property_are_mapped() {
        let edmx = schema(
            r#"<ComplexType Name="Address">
                 <Property Name="City" Type="Edm.String"/>
               </ComplexType>
               <EntityType Name="Person">
                 <Key><PropertyRef Name="ID"/></Key>
                 <Property Name="ID" Type="Edm.Int64" Nullable="false"/>
                 <Property Name="Home" Type="NS.Address"/>
                 <Property Name="Tags" Type="Collection(Edm.String)"/>
               </EntityType>
               <EntityContainer Name="C"><EntitySet Name="People" EntityType="NS.Person"/></EntityContainer>"#,
        );
        let out = generate(&edmx, "http://x").unwrap();
        assert!(out.sdl.contains("type Address {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("Home: Address"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("Tags: [String]"), "sdl:\n{}", out.sdl);
        // Int64 widens to Float; the non-nullable key is ID: Float!
        assert!(out.sdl.contains("ID: Float!"), "sdl:\n{}", out.sdl);
    }

    #[test]
    fn errors_without_entity_sets() {
        let edmx = schema(r#"<EntityType Name="Orphan"><Property Name="X" Type="Edm.String"/></EntityType>"#);
        assert_eq!(generate(&edmx, "http://x"), Err(ODataError::NoEntitySets));
    }

    #[test]
    fn errors_on_malformed_xml() {
        let err = generate("<edmx:Edmx <not xml", "http://x");
        assert!(matches!(err, Err(ODataError::InvalidXml(_))), "got: {err:?}");
    }

    #[test]
    fn output_is_deterministic() {
        let a = generate(NORTHWIND, "http://x").unwrap();
        let b = generate(NORTHWIND, "http://x").unwrap();
        assert_eq!(a, b);
    }
}
