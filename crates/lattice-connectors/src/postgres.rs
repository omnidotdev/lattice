//! Postgres ingestion: expose a Postgres database as a federated subgraph.
//!
//! [`generate`] turns an introspected schema into a GraphQL SDL and a
//! [`PostgresConnectorConfig`]: each table becomes a GraphQL type with a
//! collection query and (when it has a single-column primary key) a by-key
//! query. [`PostgresSubgraphExecutor`] resolves those at request time by running
//! SQL, letting Postgres render each row to JSON with `to_jsonb`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use async_trait::async_trait;
use lattice_config::{PostgresConnectorConfig, PostgresQueryMapping};
use lattice_execution::{ExecutionError, SubgraphError, SubgraphExecutor, SubgraphRequest, SubgraphResponse};
use serde_json::{Map, Value};
use thiserror::Error;

/// A column discovered by introspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Column name (also the GraphQL field name and the JSON key).
    pub name: String,
    /// Postgres type as `format_type` renders it, e.g. `integer`, `text`,
    /// `integer[]`, `jsonb`.
    pub pg_type: String,
    /// Whether the column is nullable.
    pub nullable: bool,
}

/// A table discovered by introspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Table name.
    pub name: String,
    /// Columns in ordinal order.
    pub columns: Vec<Column>,
    /// Primary-key column names, in key order.
    pub pk: Vec<String>,
}

/// A subgraph generated from an introspected Postgres schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSubgraph {
    /// GraphQL SDL describing the generated types and root fields.
    pub sdl: String,
    /// Postgres connector config (URL and per-field table queries).
    pub connector: PostgresConnectorConfig,
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

/// Upper-camel-case a `snake_case` table name for a GraphQL type (`blog_posts`
/// becomes `BlogPosts`).
fn pascal_case(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for part in raw.split('_').filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    sanitize_name(&out)
}

/// Map a Postgres type to a GraphQL base scalar, and whether it needs the `JSON`
/// scalar. An array type (`integer[]`) returns its element scalar.
fn scalar_for(pg_type: &str) -> (&'static str, bool) {
    let base = pg_type.strip_suffix("[]").unwrap_or(pg_type);
    match base {
        "smallint" | "integer" | "int2" | "int4" => ("Int", false),
        "bigint" | "int8" | "real" | "double precision" | "float4" | "float8" | "numeric" | "decimal" => {
            ("Float", false)
        }
        "boolean" | "bool" => ("Boolean", false),
        "json" | "jsonb" => ("JSON", true),
        _ => ("String", false),
    }
}

/// Render a column's full GraphQL type (array wrapping and nullability).
fn field_type(column: &Column, needs_json: &mut bool) -> String {
    let (base, is_json) = scalar_for(&column.pg_type);
    *needs_json |= is_json;
    let is_array = column.pg_type.ends_with("[]");
    let inner = if is_array {
        format!("[{base}!]")
    } else {
        base.to_string()
    };
    if column.nullable { inner } else { format!("{inner}!") }
}

/// Generate a GraphQL SDL and Postgres connector config from introspected tables.
#[must_use]
pub fn generate(tables: &[Table], url: &str) -> GeneratedSubgraph {
    let mut needs_json = false;
    let mut types = String::new();
    let mut queries: BTreeMap<String, String> = BTreeMap::new();
    let mut fields: BTreeMap<String, PostgresQueryMapping> = BTreeMap::new();

    for table in tables {
        let type_name = pascal_case(&table.name);
        let column_names: Vec<String> = table.columns.iter().map(|c| sanitize_name(&c.name)).collect();

        let _ = writeln!(types, "type {type_name} {{");
        for column in &table.columns {
            let _ = writeln!(
                types,
                "  {}: {}",
                sanitize_name(&column.name),
                field_type(column, &mut needs_json)
            );
        }
        types.push_str("}\n\n");

        // Collection query.
        let coll = sanitize_name(&table.name);
        queries.insert(coll.clone(), format!("{coll}: [{type_name}!]"));
        fields.insert(
            coll,
            PostgresQueryMapping {
                table: table.name.clone(),
                columns: column_names.clone(),
                pk: None,
            },
        );

        // By-primary-key query, only for a single-column key.
        if let [pk] = table.pk.as_slice() {
            let pk_type = table
                .columns
                .iter()
                .find(|c| &c.name == pk)
                .map_or("String", |c| scalar_for(&c.pg_type).0);
            let field = format!("{}_by_pk", sanitize_name(&table.name));
            queries.insert(
                field.clone(),
                format!("{field}({}: {pk_type}!): {type_name}", sanitize_name(pk)),
            );
            fields.insert(
                field,
                PostgresQueryMapping {
                    table: table.name.clone(),
                    columns: column_names,
                    pk: Some(pk.clone()),
                },
            );
        }
    }

    let mut sdl = String::new();
    if needs_json {
        sdl.push_str("scalar JSON\n\n");
    }
    sdl.push_str(&types);
    if !queries.is_empty() {
        sdl.push_str("type Query {\n");
        for line in queries.values() {
            let _ = writeln!(sdl, "  {line}");
        }
        sdl.push_str("}\n\n");
    }
    sdl.truncate(sdl.trim_end().len());
    sdl.push('\n');

    GeneratedSubgraph {
        sdl,
        connector: PostgresConnectorConfig {
            url: url.to_string(),
            fields: fields.into_iter().collect(),
        },
    }
}

// ---------------------------------------------------------------------------
// Introspection: read a live schema into the model `generate` consumes.
// ---------------------------------------------------------------------------

/// Columns of `public` base tables, views, and partitioned tables, in ordinal
/// order, with their rendered type and nullability.
const COLUMNS_SQL: &str = "\
SELECT c.relname AS tbl, a.attname AS col, \
       format_type(a.atttypid, a.atttypmod) AS typ, \
       NOT a.attnotnull AS nullable \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
WHERE n.nspname = 'public' AND c.relkind IN ('r', 'v', 'm', 'p') \
  AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY c.relname, a.attnum";

/// Primary-key columns per table, in key order.
const PRIMARY_KEYS_SQL: &str = "\
SELECT c.relname AS tbl, a.attname AS col \
FROM pg_catalog.pg_index i \
JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) \
WHERE i.indisprimary AND n.nspname = 'public' \
ORDER BY c.relname, array_position(i.indkey, a.attnum)";

/// Introspect the `public` schema of a Postgres database into the table model.
///
/// # Errors
///
/// Returns any error from the catalog queries.
pub async fn introspect(client: &tokio_postgres::Client) -> Result<Vec<Table>, tokio_postgres::Error> {
    let mut pks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in client.query(PRIMARY_KEYS_SQL, &[]).await? {
        pks.entry(row.get("tbl")).or_default().push(row.get("col"));
    }

    let mut columns: BTreeMap<String, Vec<Column>> = BTreeMap::new();
    for row in client.query(COLUMNS_SQL, &[]).await? {
        columns.entry(row.get("tbl")).or_default().push(Column {
            name: row.get("col"),
            pg_type: row.get("typ"),
            nullable: row.get("nullable"),
        });
    }

    Ok(columns
        .into_iter()
        .map(|(name, columns)| {
            let pk = pks.get(&name).cloned().unwrap_or_default();
            Table { name, columns, pk }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Runtime executor: resolve GraphQL subqueries against Postgres with SQL.
// ---------------------------------------------------------------------------

/// An error raised while running a Postgres query for a subgraph field.
#[derive(Debug, Error)]
pub enum PgExecError {
    /// The connection URL could not be parsed.
    #[error("invalid postgres url: {0}")]
    Url(String),
    /// The connection pool could not be built.
    #[error("failed to build connection pool: {0}")]
    Pool(String),
    /// A connection could not be acquired from the pool.
    #[error("failed to get a database connection: {0}")]
    Connection(String),
    /// The SQL query failed.
    #[error("query failed: {0}")]
    Query(String),
}

/// Quote a SQL identifier (doubling embedded quotes). Identifiers come from
/// introspection and the trusted config, never from client input.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// The `SELECT` column list: the quoted columns, or `*` when none are named.
fn select_list(columns: &[String]) -> String {
    if columns.is_empty() {
        "*".to_string()
    } else {
        columns.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
    }
}

/// SQL for a mapping. `to_jsonb` renders each row to JSON so Postgres performs
/// every type conversion; a by-key mapping compares the key as text against `$1`.
fn build_sql(mapping: &PostgresQueryMapping) -> String {
    let cols = select_list(&mapping.columns);
    let table = quote_ident(&mapping.table);
    mapping.pk.as_ref().map_or_else(
        || format!("SELECT to_jsonb(t) AS j FROM (SELECT {cols} FROM {table}) t"),
        |pk| {
            format!(
                "SELECT to_jsonb(t) AS j FROM (SELECT {cols} FROM {table} WHERE {}::text = $1 LIMIT 1) t",
                quote_ident(pk)
            )
        },
    )
}

/// Render a scalar argument for a text-compared key parameter.
fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Executes GraphQL subqueries by running SQL against Postgres. Implements the
/// [`SubgraphExecutor`] trait so the planner treats a Postgres source like any
/// other subgraph.
pub struct PostgresSubgraphExecutor {
    pool: deadpool_postgres::Pool,
    fields: HashMap<String, PostgresQueryMapping>,
}

impl PostgresSubgraphExecutor {
    /// Build an executor from a Postgres connector config, creating a lazy
    /// connection pool to the configured URL.
    ///
    /// # Errors
    ///
    /// Returns [`PgExecError`] if the URL is invalid or the pool cannot be built.
    pub fn from_config(config: &PostgresConnectorConfig) -> Result<Self, PgExecError> {
        let pg_config: tokio_postgres::Config = config
            .url
            .parse()
            .map_err(|e: tokio_postgres::Error| PgExecError::Url(e.to_string()))?;
        let manager = deadpool_postgres::Manager::new(pg_config, tokio_postgres::NoTls);
        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(16)
            .build()
            .map_err(|e| PgExecError::Pool(e.to_string()))?;
        Ok(Self {
            pool,
            fields: config.fields.clone(),
        })
    }

    /// Run one field's query and return its JSON (an array for a collection, a
    /// single object or null for a by-key lookup).
    async fn run_field(&self, mapping: &PostgresQueryMapping, args: &Map<String, Value>) -> Result<Value, PgExecError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| PgExecError::Connection(e.to_string()))?;
        let sql = build_sql(mapping);
        if mapping.pk.is_some() {
            let key = args.values().next().map(scalar_to_string).unwrap_or_default();
            let row = client
                .query_opt(&sql, &[&key])
                .await
                .map_err(|e| PgExecError::Query(e.to_string()))?;
            Ok(row.map_or(Value::Null, |r| r.get::<_, Value>("j")))
        } else {
            let rows = client
                .query(&sql, &[])
                .await
                .map_err(|e| PgExecError::Query(e.to_string()))?;
            Ok(Value::Array(rows.iter().map(|r| r.get::<_, Value>("j")).collect()))
        }
    }
}

#[async_trait]
impl SubgraphExecutor for PostgresSubgraphExecutor {
    async fn execute_graphql(&self, request: SubgraphRequest) -> Result<SubgraphResponse, ExecutionError> {
        let roots = crate::rest::parse_root_fields(&request.query, request.variables.as_ref())
            .map_err(|e| ExecutionError::Parse(e.to_string()))?;

        let mut data = Map::new();
        let mut errors: Vec<SubgraphError> = Vec::new();

        for root in roots {
            let Some(mapping) = self.fields.get(&root.field_name) else {
                errors.push(SubgraphError {
                    message: format!("no Postgres mapping for field '{}'", root.field_name),
                });
                data.insert(root.response_key, Value::Null);
                continue;
            };
            match self.run_field(mapping, &root.args).await {
                Ok(value) => {
                    data.insert(root.response_key, value);
                }
                Err(e) => {
                    errors.push(SubgraphError { message: e.to_string() });
                    data.insert(root.response_key, Value::Null);
                }
            }
        }

        Ok(SubgraphResponse {
            data: Some(Value::Object(data)),
            errors: (!errors.is_empty()).then_some(errors),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, pg_type: &str, nullable: bool) -> Column {
        Column {
            name: name.to_string(),
            pg_type: pg_type.to_string(),
            nullable,
        }
    }

    #[test]
    fn generates_type_collection_and_by_pk() {
        let tables = vec![Table {
            name: "users".to_string(),
            columns: vec![
                col("id", "integer", false),
                col("name", "text", true),
                col("score", "bigint", true),
            ],
            pk: vec!["id".to_string()],
        }];

        let out = generate(&tables, "postgres://localhost/app");

        assert!(out.sdl.contains("type Users {"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("id: Int!"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("name: String"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("score: Float"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("users: [Users!]"), "sdl:\n{}", out.sdl);
        assert!(out.sdl.contains("users_by_pk(id: Int!): Users"), "sdl:\n{}", out.sdl);

        assert_eq!(out.connector.url, "postgres://localhost/app");
        let coll = out.connector.fields.get("users").unwrap();
        assert_eq!(coll.table, "users");
        assert_eq!(coll.columns, vec!["id", "name", "score"]);
        assert_eq!(coll.pk, None);
        let by_pk = out.connector.fields.get("users_by_pk").unwrap();
        assert_eq!(by_pk.pk.as_deref(), Some("id"));
    }

    #[test]
    fn builds_collection_and_by_pk_sql() {
        let coll = PostgresQueryMapping {
            table: "users".to_string(),
            columns: vec!["id".to_string(), "name".to_string()],
            pk: None,
        };
        assert_eq!(
            build_sql(&coll),
            r#"SELECT to_jsonb(t) AS j FROM (SELECT "id", "name" FROM "users") t"#
        );

        let by_pk = PostgresQueryMapping {
            table: "users".to_string(),
            columns: vec!["id".to_string()],
            pk: Some("id".to_string()),
        };
        assert_eq!(
            build_sql(&by_pk),
            r#"SELECT to_jsonb(t) AS j FROM (SELECT "id" FROM "users" WHERE "id"::text = $1 LIMIT 1) t"#
        );
    }

    #[test]
    fn quotes_identifiers_safely() {
        assert_eq!(quote_ident("user"), r#""user""#);
        assert_eq!(quote_ident(r#"we"ird"#), r#""we""ird""#);
        assert_eq!(select_list(&[]), "*");
    }

    /// End-to-end against a live Postgres, gated on `LATTICE_TEST_DATABASE_URL`
    /// so it is skipped where no database is configured (for example in CI
    /// without the service).
    #[tokio::test]
    async fn introspect_generate_and_execute_against_a_live_db() {
        use lattice_execution::{SubgraphExecutor, SubgraphRequest};

        let Ok(url) = std::env::var("LATTICE_TEST_DATABASE_URL") else {
            return;
        };

        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .batch_execute(
                "DROP TABLE IF EXISTS lattice_pg_test; \
                 CREATE TABLE lattice_pg_test (id int primary key, name text, score bigint, tags text[]); \
                 INSERT INTO lattice_pg_test VALUES (1, 'a', 10, '{x,y}'), (2, 'b', 20, '{}');",
            )
            .await
            .unwrap();

        let tables = introspect(&client).await.unwrap();
        let table = tables.iter().find(|t| t.name == "lattice_pg_test").unwrap();
        assert_eq!(table.pk, vec!["id"]);
        assert!(table.columns.iter().any(|c| c.name == "name" && c.pg_type == "text"));
        assert!(table.columns.iter().any(|c| c.name == "tags" && c.pg_type == "text[]"));

        let generated = generate(&tables, &url);
        assert!(
            generated.sdl.contains("type LatticePgTest {"),
            "sdl:\n{}",
            generated.sdl
        );

        let exec = PostgresSubgraphExecutor::from_config(&generated.connector).unwrap();

        let collection = exec
            .execute_graphql(SubgraphRequest {
                url: String::new(),
                query: "{ lattice_pg_test { id name } }".to_string(),
                operation_name: None,
                variables: None,
                headers: vec![],
            })
            .await
            .unwrap();
        let rows = collection.data.unwrap();
        assert_eq!(rows["lattice_pg_test"].as_array().unwrap().len(), 2);

        let by_pk = exec
            .execute_graphql(SubgraphRequest {
                url: String::new(),
                query: "{ lattice_pg_test_by_pk(id: 1) { name score } }".to_string(),
                operation_name: None,
                variables: None,
                headers: vec![],
            })
            .await
            .unwrap();
        let row = by_pk.data.unwrap();
        assert_eq!(row["lattice_pg_test_by_pk"]["name"], serde_json::json!("a"));

        client.batch_execute("DROP TABLE lattice_pg_test").await.ok();
    }
}
