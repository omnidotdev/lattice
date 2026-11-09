# @omnidotdev/lattice

## 0.2.0

### Minor Changes

- [`982b20c`](https://github.com/omnidotdev/lattice/commit/982b20c45f0a37e33e76a885c77f5a70ec5d5184) Thanks [@coopbri](https://github.com/coopbri)! - Generate a REST subgraph directly from an OpenAPI spec. `lattice openapi <spec>` reads an OpenAPI 3.x document (JSON or YAML) and emits a GraphQL SDL plus a ready-to-paste `lattice.yaml` connector block: each operation becomes a Query or Mutation field, path and query parameters become arguments, request bodies become input types, and referenced schemas become GraphQL types. No schema or field mappings need to be hand-written.

## 0.1.0

### Minor Changes

- [`aa45468`](https://github.com/omnidotdev/lattice/commit/aa454687d93b3c335d0ba6c0b0321b61920dfbed) Thanks [@coopbri](https://github.com/coopbri)! - Initial public release: a federated GraphQL gateway written in Rust, with Federation v2 query planning and execution, WebSocket subscriptions, `@defer` support, JWT and API-key authentication with directive enforcement, entity and response caching, rate limiting, query depth/complexity/alias limits, Automatic Persisted Queries, schema contracts, a WASI plugin system, request deduplication, a native MCP endpoint, a `/health` liveness endpoint, Prometheus metrics, OpenTelemetry tracing, and a built-in GraphiQL playground.
