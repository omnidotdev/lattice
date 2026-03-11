use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use lattice_server::SharedState;
use tower::ServiceExt;

fn supergraph_sdl() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/supergraph.graphql"
    ))
    .expect("test supergraph fixture missing")
}

fn test_config_with_subgraphs(products_url: &str, reviews_url: &str) -> lattice_config::Config {
    let yaml = format!(
        r#"
server:
  listen: "0.0.0.0:4000"
supergraph:
  path: ./supergraph.graphql
subgraphs:
  products:
    url: "{products_url}"
  reviews:
    url: "{reviews_url}"
"#
    );
    serde_yaml::from_str(&yaml).unwrap()
}

#[tokio::test]
async fn full_pipeline_single_subgraph_query() {
    let mut products = mockito::Server::new_async().await;
    let products_url = format!("{}/graphql", products.url());

    let mock = products
        .mock("POST", "/graphql")
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":{"topProducts":[{"upc":"1","name":"Table","price":899}]}}"#)
        .create_async()
        .await;

    let reviews = mockito::Server::new_async().await;
    let reviews_url = format!("{}/graphql", reviews.url());

    let config = test_config_with_subgraphs(&products_url, &reviews_url);
    let sdl = supergraph_sdl();
    let state: SharedState = Arc::new(ArcSwap::from(Arc::new(lattice_server::AppState::new(
        config,
        Some(&sdl),
    ))));
    let app = lattice_server::router(state);

    // Drop servers' URL references; servers stay alive via `products` and `reviews` bindings.
    drop(products_url);
    drop(reviews_url);

    let body = serde_json::json!({
        "query": "{ topProducts { upc name price } }"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["data"]["topProducts"].is_array(),
        "expected topProducts array, got: {json}"
    );

    mock.assert_async().await;

    // Ensure mock servers are kept alive until assertions complete.
    drop(reviews);
    drop(products);
}

#[tokio::test]
async fn returns_planning_error_for_invalid_field() {
    // Build config from mock server URLs, then drop servers since no HTTP calls are needed.
    let products = mockito::Server::new_async().await;
    let products_url = format!("{}/graphql", products.url());
    drop(products);

    let reviews = mockito::Server::new_async().await;
    let reviews_url = format!("{}/graphql", reviews.url());
    drop(reviews);

    let config = test_config_with_subgraphs(&products_url, &reviews_url);
    let sdl = supergraph_sdl();
    let state: SharedState = Arc::new(ArcSwap::from(Arc::new(lattice_server::AppState::new(
        config,
        Some(&sdl),
    ))));
    let app = lattice_server::router(state);

    let body = serde_json::json!({
        "query": "{ nonExistentField }"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["errors"].is_array());
}
