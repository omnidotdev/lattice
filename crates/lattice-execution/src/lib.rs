mod dispatch;
mod executor;

pub use dispatch::{SubgraphClient, SubgraphRequest, SubgraphResponse};
pub use executor::{ExecutionResult, ExecutionResultError, PlanExecutor, deep_merge};

#[cfg(test)]
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_to_subgraph() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"me":{"id":"1","name":"Alice"}}}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let response = client
            .execute(SubgraphRequest {
                url: format!("{}/graphql", server.url()),
                query: "{ me { id name } }".into(),
                operation_name: None,
                variables: None,
                headers: vec![],
            })
            .await
            .unwrap();

        assert!(response.data.is_some());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn forwards_headers_to_subgraph() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .match_header("Authorization", "Bearer tok123")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":null}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        client
            .execute(SubgraphRequest {
                url: format!("{}/graphql", server.url()),
                query: "{ me { id } }".into(),
                operation_name: None,
                variables: None,
                headers: vec![("Authorization".into(), "Bearer tok123".into())],
            })
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn handles_subgraph_errors() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/graphql")
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":null,"errors":[{"message":"not found"}]}"#)
            .create_async()
            .await;

        let client = SubgraphClient::new();
        let response = client
            .execute(SubgraphRequest {
                url: format!("{}/graphql", server.url()),
                query: "{ me { id } }".into(),
                operation_name: None,
                variables: None,
                headers: vec![],
            })
            .await
            .unwrap();

        assert!(response.errors.is_some());
        mock.assert_async().await;
    }
}
