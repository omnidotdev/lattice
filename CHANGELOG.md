# @omnidotdev/lattice

## 0.4.0

### Minor Changes

- [#29](https://github.com/omnidotdev/lattice/pull/29) [`240e88c`](https://github.com/omnidotdev/lattice/commit/240e88cf70e749a824de78b2ae8b044176cc6e78) Thanks [@coopbri](https://github.com/coopbri)! - Add a gRPC connector: federate a gRPC service as a subgraph.

  `lattice grpc` turns a compiled protobuf `FileDescriptorSet` (from `protoc --descriptor_set_out`) into a GraphQL SDL and a `grpc` connector config block. Each service's unary methods become root fields (idempotent methods queries, the rest mutations), request and response messages become input and output types, and enums, nested messages, repeated fields, and 64-bit integers follow the proto3 JSON conventions. Streaming methods are skipped.

  At request time, a `grpc` subgraph resolves over the wire: the gateway encodes each field's input as the request message, makes the unary call, and decodes the response back into GraphQL, using the descriptor set for protobuf coding. Configure a subgraph with a `grpc` block (`endpoint`, `descriptor_set`, `fields`) just like a `rest` one.

- [#30](https://github.com/omnidotdev/lattice/pull/30) [`b895826`](https://github.com/omnidotdev/lattice/commit/b895826b0d36e4068854131b4b0951fcb594b0fc) Thanks [@coopbri](https://github.com/coopbri)! - Add `lattice odata`, which federates an OData service as a subgraph. It turns an OData 4.0 `$metadata` (EDMX) document into a GraphQL SDL and a REST connector config: each entity set becomes a collection query and a by-key query, each entity and complex type becomes a GraphQL type, and enums, collection-valued properties, 64-bit numbers, and composite keys are mapped through. Because an OData service is plain HTTP and JSON, the generated config runs on the existing REST connector unchanged. Navigation properties are skipped (they require `$expand`).

- [#32](https://github.com/omnidotdev/lattice/pull/32) [`6a3ae3a`](https://github.com/omnidotdev/lattice/commit/6a3ae3a78603bc145dc539e3b0a492662a913256) Thanks [@coopbri](https://github.com/coopbri)! - Add a Postgres connector: federate a Postgres database as a subgraph.

  `lattice postgres <url>` introspects the `public` schema and generates a GraphQL SDL plus a Postgres connector config. Each table becomes a GraphQL type, with a collection query and (for a single-column primary key) a by-key query; column nullability, arrays, and `json`/`jsonb` are mapped through, and other types fall back to `String`.

  At request time a `postgres` subgraph resolves over the wire: the connector runs `SELECT to_jsonb(...)` so Postgres renders each row to JSON and performs every type conversion itself, then shapes the result back into GraphQL. Configure a subgraph with a `postgres` block (`url`, `fields`) like a `rest` one.

  The pinned Rust toolchain is raised to 1.98 (required by the Postgres client stack).

- [#31](https://github.com/omnidotdev/lattice/pull/31) [`1eee770`](https://github.com/omnidotdev/lattice/commit/1eee770aca1a59bd7e5068b6af176e23bf0eb745) Thanks [@coopbri](https://github.com/coopbri)! - Add trusted documents (persisted operation allowlisting). Set `persisted_queries.require: true` with a `manifest`, and the gateway runs only the operations in that manifest: it preloads them, rejects freeform queries and any operation outside the set, and registers nothing at runtime. The manifest is a JSON object mapping each operation's lowercase hex SHA-256 to its document, and every entry is verified against its hash at load, so a tampered manifest is refused. Trusted mode fails closed: if the manifest cannot be read or parsed, all operations are rejected. Builds on Automatic Persisted Queries, so clients send the operation hash exactly as they would for APQ.

## 0.3.0

### Minor Changes

- [`37d3f6a`](https://github.com/omnidotdev/lattice/commit/37d3f6ac1346807bfb17f896b3e23cffa8e51c91) Thanks [@coopbri](https://github.com/coopbri)! - Add `response` field remapping to REST connectors. When an upstream's JSON field names differ from the GraphQL schema, map a GraphQL field name to a source key or JSON Pointer in the (post-`select`) response; unmapped keys pass through and arrays are remapped element by element.

## 0.2.0

### Minor Changes

- [`982b20c`](https://github.com/omnidotdev/lattice/commit/982b20c45f0a37e33e76a885c77f5a70ec5d5184) Thanks [@coopbri](https://github.com/coopbri)! - Generate a REST subgraph directly from an OpenAPI spec. `lattice openapi <spec>` reads an OpenAPI 3.x document (JSON or YAML) and emits a GraphQL SDL plus a ready-to-paste `lattice.yaml` connector block: each operation becomes a Query or Mutation field, path and query parameters become arguments, request bodies become input types, and referenced schemas become GraphQL types. No schema or field mappings need to be hand-written.

## 0.1.0

### Minor Changes

- [`aa45468`](https://github.com/omnidotdev/lattice/commit/aa454687d93b3c335d0ba6c0b0321b61920dfbed) Thanks [@coopbri](https://github.com/coopbri)! - Initial public release: a federated GraphQL gateway written in Rust, with Federation v2 query planning and execution, WebSocket subscriptions, `@defer` support, JWT and API-key authentication with directive enforcement, entity and response caching, rate limiting, query depth/complexity/alias limits, Automatic Persisted Queries, schema contracts, a WASI plugin system, request deduplication, a native MCP endpoint, a `/health` liveness endpoint, Prometheus metrics, OpenTelemetry tracing, and a built-in GraphiQL playground.
