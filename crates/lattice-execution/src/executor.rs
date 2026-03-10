use std::collections::HashMap;

use hive_router_query_planner::planner::plan_nodes::ConditionNode;
use lattice_federation::{FetchNode, FlattenNode, PlanNode, QueryPlan};
use serde_json::Value;

use crate::dispatch::{ExecutionError, SubgraphClient, SubgraphRequest};

/// Result of executing a query plan against federated subgraphs.
#[derive(Debug)]
pub struct ExecutionResult {
    pub data: Option<Value>,
    pub errors: Vec<ExecutionResultError>,
}

/// An error returned by a subgraph during execution.
#[derive(Debug)]
pub struct ExecutionResultError {
    pub message: String,
}

/// Execute federated query plans by dispatching fetches to subgraphs.
pub struct PlanExecutor<'a> {
    client: &'a SubgraphClient,
    subgraph_urls: &'a HashMap<String, String>,
}

impl<'a> PlanExecutor<'a> {
    #[must_use]
    pub const fn new(client: &'a SubgraphClient, subgraph_urls: &'a HashMap<String, String>) -> Self {
        Self { client, subgraph_urls }
    }

    /// Execute a query plan, walking the plan node tree and dispatching
    /// fetches to the appropriate subgraphs.
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError` if a subgraph request fails or a subgraph
    /// URL cannot be resolved.
    pub async fn execute(
        &self,
        plan: &QueryPlan,
        variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let Some(node) = &plan.node else {
            return Ok(ExecutionResult {
                data: None,
                errors: vec![],
            });
        };

        self.execute_node(node, None, variables).await
    }

    fn execute_node<'b>(
        &'b self,
        node: &'b PlanNode,
        parent_data: Option<&'b Value>,
        variables: Option<&'b Value>,
    ) -> futures::future::BoxFuture<'b, Result<ExecutionResult, ExecutionError>> {
        Box::pin(async move {
            match node {
                PlanNode::Fetch(fetch) => self.execute_fetch(fetch, parent_data, variables).await,
                PlanNode::Sequence(seq) => self.execute_sequence(&seq.nodes, parent_data, variables).await,
                PlanNode::Parallel(par) => self.execute_parallel(&par.nodes, parent_data, variables).await,
                PlanNode::Flatten(flatten) => self.execute_flatten(flatten, parent_data, variables).await,
                PlanNode::Condition(cond) => self.execute_condition(cond, parent_data, variables).await,
                // Subscription and Defer are not yet supported.
                PlanNode::Subscription(_) | PlanNode::Defer(_) => Ok(ExecutionResult {
                    data: parent_data.cloned(),
                    errors: vec![],
                }),
            }
        })
    }

    async fn execute_fetch(
        &self,
        fetch: &FetchNode,
        parent_data: Option<&Value>,
        client_variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let url = self
            .subgraph_urls
            .get(&fetch.service_name)
            .ok_or_else(|| ExecutionError::Parse(format!("no URL configured for subgraph `{}`", fetch.service_name)))?;

        let fetch_vars = build_fetch_variables(fetch, parent_data, client_variables);

        tracing::debug!(
            service = %fetch.service_name,
            operation = %fetch.operation.document_str,
            "dispatching fetch"
        );

        let response = self
            .client
            .execute(SubgraphRequest {
                url: url.clone(),
                query: fetch.operation.document_str.clone(),
                operation_name: fetch.operation_name.clone(),
                variables: fetch_vars,
                headers: vec![],
            })
            .await?;

        let errors = response
            .errors
            .unwrap_or_default()
            .into_iter()
            .map(|e| ExecutionResultError { message: e.message })
            .collect();

        Ok(ExecutionResult {
            data: response.data,
            errors,
        })
    }

    async fn execute_sequence(
        &self,
        nodes: &[PlanNode],
        parent_data: Option<&Value>,
        variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let mut accumulated = ExecutionResult {
            data: parent_data.cloned(),
            errors: vec![],
        };

        for node in nodes {
            let result = self.execute_node(node, accumulated.data.as_ref(), variables).await?;

            match &mut accumulated.data {
                Some(target) => {
                    if let Some(source) = result.data {
                        deep_merge(target, source);
                    }
                }
                None => accumulated.data = result.data,
            }

            accumulated.errors.extend(result.errors);
        }

        Ok(accumulated)
    }

    async fn execute_parallel(
        &self,
        nodes: &[PlanNode],
        parent_data: Option<&Value>,
        variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let futures: Vec<_> = nodes
            .iter()
            .map(|node| self.execute_node(node, parent_data, variables))
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut merged = ExecutionResult {
            data: parent_data.cloned(),
            errors: vec![],
        };

        for result in results {
            let result = result?;

            match &mut merged.data {
                Some(target) => {
                    if let Some(source) = result.data {
                        deep_merge(target, source);
                    }
                }
                None => merged.data = result.data,
            }

            merged.errors.extend(result.errors);
        }

        Ok(merged)
    }

    async fn execute_flatten(
        &self,
        flatten: &FlattenNode,
        parent_data: Option<&Value>,
        variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let result = self.execute_node(&flatten.node, parent_data, variables).await?;

        let path_segments: Vec<&str> = flatten
            .path
            .as_slice()
            .iter()
            .filter_map(|seg| seg.to_field().map(String::as_str))
            .collect();

        let mut data = parent_data.cloned().unwrap_or(Value::Null);

        if let Some(source) = &result.data {
            merge_at_path(&mut data, &path_segments, source);
        }

        Ok(ExecutionResult {
            data: Some(data),
            errors: result.errors,
        })
    }

    async fn execute_condition(
        &self,
        cond: &ConditionNode,
        parent_data: Option<&Value>,
        variables: Option<&Value>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let condition_value = variables
            .and_then(|v| v.get(&cond.condition))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if condition_value {
            if let Some(if_clause) = &cond.if_clause {
                return self.execute_node(if_clause, parent_data, variables).await;
            }
        } else if let Some(else_clause) = &cond.else_clause {
            return self.execute_node(else_clause, parent_data, variables).await;
        }

        Ok(ExecutionResult {
            data: parent_data.cloned(),
            errors: vec![],
        })
    }
}

/// Recursively merge two JSON values. For objects, keys from `source`
/// are merged into `target`. Non-null scalars in `source` overwrite
/// values in `target`.
pub fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_val) in source_map {
                let entry = target_map.entry(key).or_insert(Value::Null);
                deep_merge(entry, source_val);
            }
        }
        (target, source) => {
            if !source.is_null() {
                *target = source;
            }
        }
    }
}

/// Merge `source` data into `target` at the given nested path.
pub fn merge_at_path(target: &mut Value, path: &[&str], source: &Value) {
    if path.is_empty() {
        deep_merge(target, source.clone());
        return;
    }

    let key = path[0];
    let rest = &path[1..];

    match target {
        Value::Object(map) => {
            let entry = map.entry(key).or_insert(Value::Null);
            merge_at_path(entry, rest, source);
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                merge_at_path(item, path, source);
            }
        }
        _ => {}
    }
}

/// Build the variables object for a fetch node. Includes only variables
/// that the fetch declares in `variable_usages`, plus `representations`
/// when the fetch has a `requires` selection.
fn build_fetch_variables(
    fetch: &FetchNode,
    parent_data: Option<&Value>,
    client_variables: Option<&Value>,
) -> Option<Value> {
    let mut vars = serde_json::Map::new();

    // Include client-supplied variables that this fetch uses.
    if let (Some(usages), Some(client_vars)) = (&fetch.variable_usages, client_variables) {
        if let Some(obj) = client_vars.as_object() {
            for usage in usages {
                if let Some(val) = obj.get(usage) {
                    vars.insert(usage.clone(), val.clone());
                }
            }
        }
    }

    // Build representations for entity resolution when `requires` is set.
    if fetch.requires.is_some() {
        if let Some(data) = parent_data {
            let representations = build_representations(data);
            if !representations.is_empty() {
                vars.insert("representations".to_string(), Value::Array(representations));
            }
        }
    }

    if vars.is_empty() {
        None
    } else {
        Some(Value::Object(vars))
    }
}

/// Extract entity representations from parent data for `_entities` queries.
/// Looks for objects containing `__typename` at the top level.
fn build_representations(data: &Value) -> Vec<Value> {
    let mut representations = Vec::new();

    match data {
        Value::Object(map) => {
            // If this object itself has __typename, it is a representation.
            if map.contains_key("__typename") {
                representations.push(data.clone());
            } else {
                // Otherwise, look inside each value.
                for value in map.values() {
                    representations.extend(build_representations(value));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                representations.extend(build_representations(item));
            }
        }
        _ => {}
    }

    representations
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use hive_router_query_planner::{
        ast::{
            document::Document,
            operation::{OperationDefinition, SubgraphFetchOperation},
            selection_set::SelectionSet,
        },
        planner::plan_nodes::{FetchNode, PlanNode, QueryPlan, SequenceNode},
    };
    use serde_json::json;

    use crate::dispatch::SubgraphClient;

    use super::{PlanExecutor, deep_merge};

    /// Build a minimal `FetchNode` for tests. Only `service_name` and
    /// `document_str` matter during execution; the AST fields can be
    /// empty stubs.
    fn test_fetch_node(service_name: &str, document_str: &str) -> FetchNode {
        FetchNode {
            id: 0,
            service_name: service_name.to_string(),
            variable_usages: None,
            operation_kind: None,
            operation_name: None,
            operation: SubgraphFetchOperation {
                document: Document {
                    operation: OperationDefinition {
                        name: None,
                        operation_kind: None,
                        selection_set: SelectionSet::default(),
                        variable_definitions: None,
                    },
                    fragments: vec![],
                },
                document_str: document_str.to_string(),
                hash: 0,
            },
            requires: None,
            input_rewrites: None,
            output_rewrites: None,
        }
    }

    #[tokio::test]
    async fn executes_single_fetch() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"me":{"id":"1","name":"Alice"}}}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let urls: HashMap<String, String> = [("users".into(), format!("{}/graphql", server.url()))]
            .into_iter()
            .collect();

        let plan = QueryPlan {
            kind: "QueryPlan",
            node: Some(PlanNode::Fetch(test_fetch_node("users", "{ me { id name } }"))),
        };

        let executor = PlanExecutor::new(&client, &urls);
        let result = executor.execute(&plan, None).await.unwrap();

        assert_eq!(result.data, Some(json!({"me": {"id": "1", "name": "Alice"}})));
        assert!(result.errors.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn executes_sequence() {
        let mut server = mockito::Server::new_async().await;

        // First fetch returns partial data.
        let mock1 = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"me":{"id":"1"}}}"#)
            .create_async()
            .await;

        // Second fetch returns additional data.
        let mock2 = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"me":{"name":"Bob"}}}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let urls: HashMap<String, String> = [("users".into(), format!("{}/graphql", server.url()))]
            .into_iter()
            .collect();

        let plan = QueryPlan {
            kind: "QueryPlan",
            node: Some(PlanNode::Sequence(SequenceNode {
                nodes: vec![
                    PlanNode::Fetch(test_fetch_node("users", "{ me { id } }")),
                    PlanNode::Fetch(test_fetch_node("users", "{ me { name } }")),
                ],
            })),
        };

        let executor = PlanExecutor::new(&client, &urls);
        let result = executor.execute(&plan, None).await.unwrap();

        assert_eq!(result.data, Some(json!({"me": {"id": "1", "name": "Bob"}})));
        mock1.assert_async().await;
        mock2.assert_async().await;
    }

    #[tokio::test]
    async fn returns_empty_for_no_node() {
        let client = SubgraphClient::new();
        let urls = HashMap::new();
        let plan = QueryPlan {
            kind: "QueryPlan",
            node: None,
        };

        let executor = PlanExecutor::new(&client, &urls);
        let result = executor.execute(&plan, None).await.unwrap();

        assert!(result.data.is_none());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn deep_merge_combines_objects() {
        let mut target = json!({"a": 1, "b": {"x": 10}});
        let source = json!({"b": {"y": 20}, "c": 3});

        deep_merge(&mut target, source);

        assert_eq!(target, json!({"a": 1, "b": {"x": 10, "y": 20}, "c": 3}));
    }

    #[test]
    fn deep_merge_source_overwrites_scalars() {
        let mut target = json!({"a": 1});
        let source = json!({"a": 2});

        deep_merge(&mut target, source);

        assert_eq!(target, json!({"a": 2}));
    }

    #[test]
    fn deep_merge_ignores_null_source() {
        let mut target = json!({"a": 1});
        let source = json!({"a": null});

        deep_merge(&mut target, source);

        assert_eq!(target, json!({"a": 1}));
    }

    #[tokio::test]
    async fn collects_subgraph_errors() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":null,"errors":[{"message":"not found"}]}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let urls: HashMap<String, String> = [("users".into(), format!("{}/graphql", server.url()))]
            .into_iter()
            .collect();

        let plan = QueryPlan {
            kind: "QueryPlan",
            node: Some(PlanNode::Fetch(test_fetch_node("users", "{ me { id } }"))),
        };

        let executor = PlanExecutor::new(&client, &urls);
        let result = executor.execute(&plan, None).await.unwrap();

        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].message, "not found");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn filters_variables_by_usage() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"variables":{"id":"42"}}"#.to_string(),
            ))
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"user":{"id":"42"}}}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let urls: HashMap<String, String> = [("users".into(), format!("{}/graphql", server.url()))]
            .into_iter()
            .collect();

        let mut fetch = test_fetch_node("users", "query($id: ID!) { user(id: $id) { id } }");
        fetch.variable_usages = Some(BTreeSet::from(["id".to_string()]));

        let plan = QueryPlan {
            kind: "QueryPlan",
            node: Some(PlanNode::Fetch(fetch)),
        };

        let vars = json!({"id": "42", "unused": true});
        let executor = PlanExecutor::new(&client, &urls);
        let result = executor.execute(&plan, Some(&vars)).await.unwrap();

        assert!(result.data.is_some());
        mock.assert_async().await;
    }
}
