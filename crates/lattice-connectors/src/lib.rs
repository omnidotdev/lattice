//! Non-GraphQL source connectors for the Lattice gateway.
//!
//! A connector adapts an upstream that does not speak GraphQL (starting with
//! REST/HTTP) into a federated subgraph. The planner treats the source like any
//! other subgraph; a connector translates the planner's subquery into the
//! upstream protocol and shapes the response back into GraphQL.

pub mod grpc;
pub mod odata;
pub mod openapi;
pub mod rest;
