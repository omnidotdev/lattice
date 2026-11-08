//! REST connector: translating GraphQL subgraph operations into REST calls.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use lattice_config::{RestConnectorConfig, RestFieldMapping};
use lattice_execution::{ExecutionError, SubgraphError, SubgraphExecutor, SubgraphRequest, SubgraphResponse};
use serde_json::{Map, Value};

/// An error raised while building a REST request from a field mapping.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RestError {
    /// A `{name}` placeholder in the path template had no matching argument.
    #[error("missing argument for path placeholder: {0}")]
    MissingPathArgument(String),
    /// The composed base URL plus path did not parse as a valid URL.
    #[error("invalid request URL: {0}")]
    InvalidUrl(String),
    /// The subgraph operation could not be parsed as GraphQL.
    #[error("failed to parse subgraph operation: {0}")]
    Parse(String),
    /// The upstream HTTP call failed or returned an unreadable body.
    #[error("upstream HTTP request failed: {0}")]
    Http(String),
}

/// A concrete HTTP request derived from a field mapping and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestHttpRequest {
    /// HTTP method, uppercased.
    pub method: String,
    /// Fully composed request URL, including query string.
    pub url: String,
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// Optional JSON request body (set for write methods via the mapping `body`
    /// argument).
    pub body: Option<Value>,
}

/// Build a concrete HTTP request for a GraphQL root field.
///
/// Appends the interpolated `path` to `base_url`, adds the mapped query
/// parameters whose arguments are present, and attaches `headers`.
///
/// # Errors
///
/// Returns [`RestError::MissingPathArgument`] for an unfilled path placeholder,
/// or [`RestError::InvalidUrl`] if the composed URL does not parse.
pub fn build_request(
    base_url: &str,
    headers: &[(String, String)],
    mapping: &RestFieldMapping,
    args: &Map<String, Value>,
) -> Result<RestHttpRequest, RestError> {
    let path = build_path(&mapping.path, args)?;
    let mut url =
        reqwest::Url::parse(&format!("{base_url}{path}")).map_err(|e| RestError::InvalidUrl(e.to_string()))?;

    let query: Vec<(&str, String)> = mapping
        .query
        .iter()
        .filter_map(|(param, arg_name)| {
            args.get(arg_name)
                .map(|value| (param.as_str(), scalar_to_string(value)))
        })
        .collect();
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (param, value) in &query {
            pairs.append_pair(param, value);
        }
    }

    Ok(RestHttpRequest {
        method: mapping.method.to_uppercase(),
        url: url.to_string(),
        headers: headers.to_vec(),
        body: mapping.body.as_ref().and_then(|arg| args.get(arg).cloned()),
    })
}

/// Interpolate `{name}` placeholders in a path template from GraphQL field
/// arguments. An unmatched `{` is kept literal.
///
/// # Errors
///
/// Returns [`RestError::MissingPathArgument`] if a placeholder has no matching
/// argument.
pub fn build_path(template: &str, args: &Map<String, Value>) -> Result<String, RestError> {
    let mut out = String::with_capacity(template.len());
    let mut remaining = template;

    loop {
        let Some(open) = remaining.find('{') else {
            out.push_str(remaining);
            break;
        };
        out.push_str(&remaining[..open]);
        let after = &remaining[open + 1..];

        let Some(close) = after.find('}') else {
            // Unmatched brace: keep the remainder literal and stop scanning.
            out.push('{');
            out.push_str(after);
            break;
        };

        let name = &after[..close];
        let value = args
            .get(name)
            .ok_or_else(|| RestError::MissingPathArgument(name.to_string()))?;
        out.push_str(&scalar_to_string(value));
        remaining = &after[close + 1..];
    }

    Ok(out)
}

/// One top-level field selected from a subgraph operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootField {
    /// Response key (the field alias if present, else the field name).
    pub response_key: String,
    /// The schema field name, used to look up its REST mapping.
    pub field_name: String,
    /// Field arguments with variables resolved against the request variables.
    pub args: Map<String, Value>,
}

/// Parse a subgraph operation and return its top-level fields with arguments
/// resolved. `$variable` arguments are looked up in `variables`.
///
/// # Errors
///
/// Returns [`RestError::Parse`] if the operation is not valid GraphQL.
pub fn parse_root_fields(query: &str, variables: Option<&Value>) -> Result<Vec<RootField>, RestError> {
    use graphql_tools::parser::query::{Definition, OperationDefinition, Selection};

    let document = graphql_tools::parser::parse_query::<String>(query).map_err(|e| RestError::Parse(e.to_string()))?;

    let mut roots = Vec::new();
    for definition in document.definitions {
        let Definition::Operation(operation) = definition else {
            continue;
        };
        let selection_set = match operation {
            OperationDefinition::SelectionSet(set) => set,
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::Mutation(mutation) => mutation.selection_set,
            OperationDefinition::Subscription(subscription) => subscription.selection_set,
        };
        for selection in selection_set.items {
            let Selection::Field(field) = selection else {
                continue;
            };
            let response_key = field.alias.clone().unwrap_or_else(|| field.name.clone());
            let mut args = Map::new();
            for (name, value) in field.arguments {
                args.insert(name, gql_value_to_json(value, variables));
            }
            roots.push(RootField {
                response_key,
                field_name: field.name,
                args,
            });
        }
    }
    Ok(roots)
}

/// Convert a parsed GraphQL argument value into JSON, resolving `$variable`
/// references against the request variables (an unknown variable becomes null).
fn gql_value_to_json(value: graphql_tools::parser::query::Value<String>, variables: Option<&Value>) -> Value {
    use graphql_tools::parser::query::Value as GqlValue;

    match value {
        GqlValue::Variable(name) => variables
            .and_then(|vars| vars.get(&name))
            .cloned()
            .unwrap_or(Value::Null),
        GqlValue::Int(number) => number.as_i64().map_or(Value::Null, Value::from),
        GqlValue::Float(float) => serde_json::Number::from_f64(float).map_or(Value::Null, Value::Number),
        GqlValue::String(string) => Value::String(string),
        GqlValue::Boolean(boolean) => Value::Bool(boolean),
        GqlValue::Null => Value::Null,
        GqlValue::Enum(variant) => Value::String(variant),
        GqlValue::List(items) => Value::Array(
            items
                .into_iter()
                .map(|item| gql_value_to_json(item, variables))
                .collect(),
        ),
        GqlValue::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, gql_value_to_json(value, variables)))
                .collect(),
        ),
    }
}

/// Render a scalar argument value for use in a URL path or query string. String
/// values are emitted without their JSON quotes; `null` renders as empty.
fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Select the field result from a REST response body. With a `select` JSON
/// Pointer, return the pointed-to value (or null if absent); otherwise return
/// the whole body.
fn select_value(body: &Value, select: Option<&str>) -> Value {
    select.map_or_else(
        || body.clone(),
        |pointer| body.pointer(pointer).cloned().unwrap_or(Value::Null),
    )
}

/// Performs the upstream HTTP call for a REST connector. Abstracted so the
/// executor can be exercised without a live network.
#[async_trait]
pub trait HttpCaller: Send + Sync {
    /// Send `request` and return the parsed JSON response body.
    ///
    /// # Errors
    ///
    /// Returns [`RestError::Http`] if the request fails or the body is not JSON.
    async fn call(&self, request: &RestHttpRequest) -> Result<Value, RestError>;
}

/// The production [`HttpCaller`], backed by `reqwest`.
pub struct ReqwestCaller {
    client: reqwest::Client,
}

impl ReqwestCaller {
    /// Create a caller with a fresh HTTP client.
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestCaller {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HttpCaller for ReqwestCaller {
    async fn call(&self, request: &RestHttpRequest) -> Result<Value, RestError> {
        let method =
            reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|e| RestError::Http(e.to_string()))?;
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        let response = builder.send().await.map_err(|e| RestError::Http(e.to_string()))?;
        response
            .json::<Value>()
            .await
            .map_err(|e| RestError::Http(e.to_string()))
    }
}

/// A subgraph backed by a REST upstream.
///
/// Implements [`SubgraphExecutor`] so the planner can route fetches to it like
/// any other subgraph; each root field is translated into an HTTP call via its
/// configured mapping.
pub struct RestSubgraphExecutor {
    base_url: String,
    headers: Vec<(String, String)>,
    fields: HashMap<String, RestFieldMapping>,
    entities: HashMap<String, RestFieldMapping>,
    caller: Arc<dyn HttpCaller>,
}

impl RestSubgraphExecutor {
    /// Construct an executor from its parts (the `caller` is injectable for
    /// testing). Entity resolvers default to empty; add them with
    /// [`Self::with_entities`].
    #[must_use]
    pub fn new(
        base_url: String,
        headers: Vec<(String, String)>,
        fields: HashMap<String, RestFieldMapping>,
        caller: Arc<dyn HttpCaller>,
    ) -> Self {
        Self {
            base_url,
            headers,
            fields,
            entities: HashMap::new(),
            caller,
        }
    }

    /// Set the entity resolvers (keyed by GraphQL type name) used for `@key`
    /// (`_entities`) resolution.
    #[must_use]
    pub fn with_entities(mut self, entities: HashMap<String, RestFieldMapping>) -> Self {
        self.entities = entities;
        self
    }

    /// Build an executor from a connector config, using the production HTTP
    /// caller.
    #[must_use]
    pub fn from_config(config: &RestConnectorConfig) -> Self {
        let headers = config
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Self::new(
            config.base_url.clone(),
            headers,
            config.fields.clone(),
            Arc::new(ReqwestCaller::new()),
        )
        .with_entities(config.entities.clone())
    }

    /// Resolve a batch of federated entity representations over REST, returning
    /// the `_entities` array (one resolved entity per representation, in order).
    async fn resolve_entities(&self, args: &Map<String, Value>, errors: &mut Vec<SubgraphError>) -> Value {
        let Some(representations) = args.get("representations").and_then(Value::as_array) else {
            return Value::Array(Vec::new());
        };
        let mut resolved = Vec::with_capacity(representations.len());
        for representation in representations {
            resolved.push(self.resolve_one_entity(representation, errors).await);
        }
        Value::Array(resolved)
    }

    /// Resolve a single entity representation (`{ __typename, ...keys }`) via its
    /// configured REST resolver, filling the resolver `path` from the key fields.
    async fn resolve_one_entity(&self, representation: &Value, errors: &mut Vec<SubgraphError>) -> Value {
        let Some(typename) = representation.get("__typename").and_then(Value::as_str) else {
            errors.push(SubgraphError {
                message: "entity representation is missing __typename".to_string(),
            });
            return Value::Null;
        };
        let Some(mapping) = self.entities.get(typename) else {
            errors.push(SubgraphError {
                message: format!("no REST entity resolver for type '{typename}'"),
            });
            return Value::Null;
        };
        let empty = Map::new();
        let key_fields = representation.as_object().unwrap_or(&empty);

        let request = match build_request(&self.base_url, &self.headers, mapping, key_fields) {
            Ok(request) => request,
            Err(e) => {
                errors.push(SubgraphError { message: e.to_string() });
                return Value::Null;
            }
        };
        match self.caller.call(&request).await {
            Ok(body) => select_value(&body, mapping.select.as_deref()),
            Err(e) => {
                errors.push(SubgraphError { message: e.to_string() });
                Value::Null
            }
        }
    }
}

#[async_trait]
impl SubgraphExecutor for RestSubgraphExecutor {
    async fn execute_graphql(&self, request: SubgraphRequest) -> Result<SubgraphResponse, ExecutionError> {
        let roots = parse_root_fields(&request.query, request.variables.as_ref())
            .map_err(|e| ExecutionError::Parse(e.to_string()))?;

        let mut data = Map::new();
        let mut errors: Vec<SubgraphError> = Vec::new();

        for root in roots {
            // Federated entity resolution: `_entities(representations: [...])`.
            if root.field_name == "_entities" {
                let value = self.resolve_entities(&root.args, &mut errors).await;
                data.insert(root.response_key, value);
                continue;
            }

            let Some(mapping) = self.fields.get(&root.field_name) else {
                errors.push(SubgraphError {
                    message: format!("no REST mapping for field '{}'", root.field_name),
                });
                data.insert(root.response_key, Value::Null);
                continue;
            };

            let http_request = match build_request(&self.base_url, &self.headers, mapping, &root.args) {
                Ok(request) => request,
                Err(e) => {
                    errors.push(SubgraphError { message: e.to_string() });
                    data.insert(root.response_key, Value::Null);
                    continue;
                }
            };

            match self.caller.call(&http_request).await {
                Ok(body) => {
                    data.insert(root.response_key, select_value(&body, mapping.select.as_deref()));
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
    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;

    fn mapping(method: &str, path: &str) -> RestFieldMapping {
        RestFieldMapping {
            method: method.to_string(),
            path: path.to_string(),
            query: HashMap::new(),
            select: None,
            body: None,
        }
    }

    fn query_pairs(url: &str) -> Vec<(String, String)> {
        let parsed = reqwest::Url::parse(url).unwrap();
        let mut pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        pairs.sort();
        pairs
    }

    #[test]
    fn interpolates_path_placeholders_from_args() {
        let mut args = Map::new();
        args.insert("id".to_string(), Value::String("42".to_string()));

        let path = build_path("/users/{id}", &args).unwrap();

        assert_eq!(path, "/users/42");
    }

    #[test]
    fn renders_numeric_argument_without_quotes() {
        let mut args = Map::new();
        args.insert("id".to_string(), Value::from(42));

        let path = build_path("/users/{id}", &args).unwrap();

        assert_eq!(path, "/users/42");
    }

    #[test]
    fn errors_on_missing_path_argument() {
        let args = Map::new();

        let err = build_path("/users/{id}", &args).unwrap_err();

        assert_eq!(err, RestError::MissingPathArgument("id".to_string()));
    }

    #[test]
    fn keeps_an_unmatched_brace_literal() {
        let args = Map::new();

        let path = build_path("/a/{unclosed", &args).unwrap();

        assert_eq!(path, "/a/{unclosed");
    }

    #[test]
    fn builds_url_from_base_and_path() {
        let mut args = Map::new();
        args.insert("id".to_string(), Value::from(7));

        let req = build_request("https://api.example.com", &[], &mapping("GET", "/pets/{id}"), &args).unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://api.example.com/pets/7");
        assert!(req.headers.is_empty());
    }

    #[test]
    fn uppercases_method() {
        let args = Map::new();

        let req = build_request("https://api.example.com", &[], &mapping("post", "/pets"), &args).unwrap();

        assert_eq!(req.method, "POST");
    }

    #[test]
    fn appends_mapped_query_params_present_in_args() {
        let mut map = mapping("GET", "/pets");
        map.query.insert("status".to_string(), "state".to_string());
        map.query.insert("limit".to_string(), "count".to_string());
        let mut args = Map::new();
        args.insert("state".to_string(), Value::String("available".to_string()));
        args.insert("count".to_string(), Value::from(10));

        let req = build_request("https://api.example.com", &[], &map, &args).unwrap();

        assert_eq!(
            query_pairs(&req.url),
            vec![
                ("limit".to_string(), "10".to_string()),
                ("status".to_string(), "available".to_string()),
            ]
        );
    }

    #[test]
    fn omits_query_param_when_its_argument_is_absent() {
        let mut map = mapping("GET", "/pets");
        map.query.insert("status".to_string(), "state".to_string());
        let args = Map::new();

        let req = build_request("https://api.example.com", &[], &map, &args).unwrap();

        assert_eq!(req.url, "https://api.example.com/pets");
    }

    #[test]
    fn attaches_headers() {
        let args = Map::new();
        let headers = vec![("Authorization".to_string(), "Bearer t".to_string())];

        let req = build_request("https://api.example.com", &headers, &mapping("GET", "/pets"), &args).unwrap();

        assert_eq!(req.headers, headers);
    }

    #[test]
    fn parses_a_single_root_field_with_a_literal_argument() {
        let roots = parse_root_fields(r#"{ pet(id: "7") { name } }"#, None).unwrap();

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].response_key, "pet");
        assert_eq!(roots[0].field_name, "pet");
        assert_eq!(roots[0].args.get("id"), Some(&Value::String("7".to_string())));
    }

    #[test]
    fn resolves_variable_arguments_from_variables() {
        let variables = serde_json::json!({ "id": "7" });

        let roots = parse_root_fields("query GetPet($id: ID!) { pet(id: $id) { name } }", Some(&variables)).unwrap();

        assert_eq!(roots[0].args.get("id"), Some(&Value::String("7".to_string())));
    }

    #[test]
    fn uses_the_alias_as_the_response_key() {
        let roots = parse_root_fields(r#"{ p: pet(id: "1") { name } }"#, None).unwrap();

        assert_eq!(roots[0].response_key, "p");
        assert_eq!(roots[0].field_name, "pet");
    }

    #[test]
    fn parses_numeric_arguments_as_numbers() {
        let roots = parse_root_fields("{ pets(limit: 5) { id } }", None).unwrap();

        assert_eq!(roots[0].args.get("limit"), Some(&Value::from(5)));
    }

    #[test]
    fn parses_multiple_root_fields() {
        let roots = parse_root_fields(r#"{ a: pet(id: "1") { name } b: pet(id: "2") { name } }"#, None).unwrap();

        let keys: Vec<&str> = roots.iter().map(|r| r.response_key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn errors_on_invalid_graphql() {
        let err = parse_root_fields("{ this is not valid", None).unwrap_err();

        assert!(matches!(err, RestError::Parse(_)));
    }

    #[test]
    fn select_value_returns_whole_body_without_a_pointer() {
        let body = serde_json::json!({ "id": "7" });

        assert_eq!(select_value(&body, None), body);
    }

    #[test]
    fn select_value_plucks_a_pointer() {
        let body = serde_json::json!({ "data": { "id": "7" } });

        assert_eq!(select_value(&body, Some("/data")), serde_json::json!({ "id": "7" }));
    }

    #[test]
    fn select_value_is_null_for_a_missing_pointer() {
        let body = serde_json::json!({ "id": "7" });

        assert_eq!(select_value(&body, Some("/missing")), Value::Null);
    }

    struct FakeCaller {
        body: Value,
        calls: Mutex<Vec<RestHttpRequest>>,
    }

    impl FakeCaller {
        fn new(body: Value) -> Self {
            Self {
                body,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn last(&self) -> RestHttpRequest {
            self.calls.lock().unwrap().last().unwrap().clone()
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl HttpCaller for FakeCaller {
        async fn call(&self, request: &RestHttpRequest) -> Result<Value, RestError> {
            self.calls.lock().unwrap().push(request.clone());
            Ok(self.body.clone())
        }
    }

    fn subgraph_request(query: &str, variables: Option<Value>) -> SubgraphRequest {
        SubgraphRequest {
            url: String::new(),
            query: query.to_string(),
            operation_name: None,
            variables,
            headers: vec![],
        }
    }

    #[tokio::test]
    async fn executes_a_rest_field_end_to_end() {
        let mut fields = HashMap::new();
        fields.insert("pet".to_string(), mapping("GET", "/pets/{id}"));
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "id": "7", "name": "Rex" })));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], fields, caller.clone());

        let response = executor
            .execute_graphql(subgraph_request(r#"{ pet(id: "7") { id name } }"#, None))
            .await
            .unwrap();

        assert_eq!(caller.last().url, "https://api.example.com/pets/7");
        assert_eq!(
            response.data,
            Some(serde_json::json!({ "pet": { "id": "7", "name": "Rex" } }))
        );
        assert!(response.errors.is_none());
    }

    #[tokio::test]
    async fn resolves_variables_before_calling_rest() {
        let mut fields = HashMap::new();
        fields.insert("pet".to_string(), mapping("GET", "/pets/{id}"));
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "id": "9" })));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], fields, caller.clone());

        executor
            .execute_graphql(subgraph_request(
                "query P($id: ID!) { pet(id: $id) { id } }",
                Some(serde_json::json!({ "id": "9" })),
            ))
            .await
            .unwrap();

        assert_eq!(caller.last().url, "https://api.example.com/pets/9");
    }

    #[tokio::test]
    async fn applies_the_select_pointer_to_the_response() {
        let mut map = mapping("GET", "/pets/{id}");
        map.select = Some("/data".to_string());
        let mut fields = HashMap::new();
        fields.insert("pet".to_string(), map);
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "data": { "id": "7" } })));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], fields, caller);

        let response = executor
            .execute_graphql(subgraph_request(r#"{ pet(id: "7") { id } }"#, None))
            .await
            .unwrap();

        assert_eq!(response.data, Some(serde_json::json!({ "pet": { "id": "7" } })));
    }

    #[tokio::test]
    async fn reports_an_error_for_an_unmapped_field() {
        let caller = Arc::new(FakeCaller::new(Value::Null));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], HashMap::new(), caller);

        let response = executor
            .execute_graphql(subgraph_request(r#"{ pet(id: "1") { id } }"#, None))
            .await
            .unwrap();

        assert_eq!(response.data, Some(serde_json::json!({ "pet": null })));
        let errors = response.errors.unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("pet"));
    }

    #[test]
    fn build_request_includes_the_body_argument() {
        let mut map = mapping("POST", "/pets");
        map.body = Some("input".to_string());
        let mut args = Map::new();
        args.insert("input".to_string(), serde_json::json!({ "name": "Rex" }));

        let req = build_request("https://api.example.com", &[], &map, &args).unwrap();

        assert_eq!(req.method, "POST");
        assert_eq!(req.body, Some(serde_json::json!({ "name": "Rex" })));
    }

    #[tokio::test]
    async fn executes_a_mutation_with_a_request_body() {
        let mut create = mapping("POST", "/pets");
        create.body = Some("input".to_string());
        let mut fields = HashMap::new();
        fields.insert("createPet".to_string(), create);
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "id": "7", "name": "Rex" })));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], fields, caller.clone());

        let response = executor
            .execute_graphql(subgraph_request(
                r#"mutation { createPet(input: { name: "Rex" }) { id } }"#,
                None,
            ))
            .await
            .unwrap();

        let last = caller.last();
        assert_eq!(last.method, "POST");
        assert_eq!(last.url, "https://api.example.com/pets");
        assert_eq!(last.body, Some(serde_json::json!({ "name": "Rex" })));
        assert_eq!(
            response.data,
            Some(serde_json::json!({ "createPet": { "id": "7", "name": "Rex" } }))
        );
    }

    #[tokio::test]
    async fn resolves_an_entity_over_rest() {
        let mut entities = HashMap::new();
        entities.insert("Pet".to_string(), mapping("GET", "/pets/{id}"));
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "id": "7", "name": "Rex" })));
        let executor = RestSubgraphExecutor::new(
            "https://api.example.com".to_string(),
            vec![],
            HashMap::new(),
            caller.clone(),
        )
        .with_entities(entities);

        let response = executor
            .execute_graphql(subgraph_request(
                "query($representations: [_Any!]!) { _entities(representations: $representations) { ... on Pet { name } } }",
                Some(serde_json::json!({ "representations": [{ "__typename": "Pet", "id": "7" }] })),
            ))
            .await
            .unwrap();

        assert_eq!(caller.last().url, "https://api.example.com/pets/7");
        assert_eq!(
            response.data,
            Some(serde_json::json!({ "_entities": [{ "id": "7", "name": "Rex" }] }))
        );
    }

    #[tokio::test]
    async fn resolves_multiple_entities_in_order() {
        let mut entities = HashMap::new();
        entities.insert("Pet".to_string(), mapping("GET", "/pets/{id}"));
        let caller = Arc::new(FakeCaller::new(serde_json::json!({ "ok": true })));
        let executor = RestSubgraphExecutor::new(
            "https://api.example.com".to_string(),
            vec![],
            HashMap::new(),
            caller.clone(),
        )
        .with_entities(entities);

        let response = executor
            .execute_graphql(subgraph_request(
                "query($representations: [_Any!]!) { _entities(representations: $representations) { __typename } }",
                Some(serde_json::json!({ "representations": [
                    { "__typename": "Pet", "id": "1" },
                    { "__typename": "Pet", "id": "2" }
                ] })),
            ))
            .await
            .unwrap();

        assert_eq!(caller.call_count(), 2);
        assert_eq!(response.data.unwrap()["_entities"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn errors_when_no_entity_resolver_matches() {
        let caller = Arc::new(FakeCaller::new(Value::Null));
        let executor = RestSubgraphExecutor::new("https://api.example.com".to_string(), vec![], HashMap::new(), caller);

        let response = executor
            .execute_graphql(subgraph_request(
                "query($representations: [_Any!]!) { _entities(representations: $representations) { __typename } }",
                Some(serde_json::json!({ "representations": [{ "__typename": "Pet", "id": "1" }] })),
            ))
            .await
            .unwrap();

        assert_eq!(response.data, Some(serde_json::json!({ "_entities": [null] })));
        assert!(response.errors.unwrap()[0].message.contains("Pet"));
    }
}
