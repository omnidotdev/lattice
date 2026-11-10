---
"@omnidotdev/lattice": minor
---

Add a Postgres connector: federate a Postgres database as a subgraph.

`lattice postgres <url>` introspects the `public` schema and generates a GraphQL SDL plus a Postgres connector config. Each table becomes a GraphQL type, with a collection query and (for a single-column primary key) a by-key query; column nullability, arrays, and `json`/`jsonb` are mapped through, and other types fall back to `String`.

At request time a `postgres` subgraph resolves over the wire: the connector runs `SELECT to_jsonb(...)` so Postgres renders each row to JSON and performs every type conversion itself, then shapes the result back into GraphQL. Configure a subgraph with a `postgres` block (`url`, `fields`) like a `rest` one.

The pinned Rust toolchain is raised to 1.98 (required by the Postgres client stack).
