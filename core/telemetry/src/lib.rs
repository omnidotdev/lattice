#![allow(unused_crate_dependencies)]
//! Lattice [tracing](https://docs.rs/tracing/latest/tracing/) integration

pub use gateway_config::telemetry as config;
/// Potential errors from this crate
pub mod error;
pub mod client;
pub mod graphql;
pub mod http;
pub mod metrics;
/// Otel integration
pub mod otel;
/// Spans that are represented using types
pub mod span;

pub(crate) const SCOPE: &str = "lattice";
pub(crate) const SCOPE_VERSION: &str = "1.0";
