# Lattice Clean-Room Design

GraphQL federation gateway for the Omni ecosystem. Clean-room implementation — no forked code, MIT-licensed dependencies only.

## Architecture Overview

```
                       ┌─────────────────────────────────────────────┐
                       │              LATTICE GATEWAY                │
  Client ─────────────▶│                                             │
                       │  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
                       │  │  Auth   │  │   Rate    │  │   WASI    │  │
                       │  │  (JWT)  │  │  Limiter  │  │  Plugins  │  │
                       │  └────┬────┘  └────┬─────┘  └─────┬─────┘  │
                       │       │            │               │        │
                       │  ┌────▼────────────▼───────────────▼─────┐  │
                       │  │           Request Pipeline            │  │
                       │  └────────────────┬──────────────────────┘  │
                       │                   │                         │
                       │  ┌────────────────▼──────────────────────┐  │
                       │  │         Query Planner                 │  │
                       │  │   (hive-router-query-planner, MIT)    │  │
                       │  └────────────────┬──────────────────────┘  │
                       │                   │                         │
                       │  ┌────────────────▼──────────────────────┐  │
                       │  │         Plan Executor                 │  │
                       │  │   (hive-router-plan-executor, MIT)    │  │
                       │  └────┬──────────┬──────────────┬────────┘  │
                       │       │          │              │           │
                       └───────┼──────────┼──────────────┼───────────┘
                               │          │              │
                        ┌──────▼──┐ ┌─────▼────┐  ┌─────▼────┐
                        │ Aether  │ │Gatekeeper│  │  Runa    │  ...
                        │subgraph │ │ subgraph │  │ subgraph │
                        └─────────┘ └──────────┘  └──────────┘
```

## Composition Strategy

Schema composition happens **offline at build/deploy time**, not at runtime.

**Build step (MIT):** `@theguild/federation-composition` (npm) composes subgraph schemas into a `supergraph.graphql` file containing `@join__*` directives.

**Runtime (MIT):** Lattice loads the pre-composed supergraph SDL. `hive-router-query-planner` parses it and builds an internal graph for query planning. No composition logic in the Rust codebase.

**CLI command:** `lattice compose --config lattice.yaml --output supergraph.graphql`

This wraps the npm composition library, keeping the Rust binary thin and avoiding any MPL-2.0 dependencies.

## Crate Architecture

```
lattice/
├── crates/
│   ├── lattice-gateway/       # [[bin]] server binary (thin main.rs)
│   ├── lattice-cli/           # [[bin]] CLI (compose, serve, check)
│   ├── lattice-server/        # axum server, request pipeline, lifecycle hooks
│   ├── lattice-federation/    # supergraph loading, query planning (wraps hive crates)
│   ├── lattice-execution/     # plan execution, subgraph HTTP dispatch
│   ├── lattice-auth/          # JWT validation, @authenticated/@authorized enforcement
│   ├── lattice-cache/         # entity caching with Valkey backend
│   ├── lattice-rate-limit/    # rate limiting with Valkey backend
│   ├── lattice-plugin/        # Extism plugin runtime (consistent with Vortex/ODK)
│   ├── lattice-config/        # YAML config parsing + validation
│   └── lattice-telemetry/     # tracing + OpenTelemetry export
├── connectors/
│   ├── postgres/              # first-party Postgres connector (WASI)
│   ├── openapi/               # first-party OpenAPI connector (WASI)
│   └── graphql/               # first-party GraphQL proxy connector (WASI)
├── plugins/
│   └── example-auth/          # example WASI auth plugin
├── Cargo.toml
├── Dockerfile
└── lattice.yaml               # example config
```

### Crate Responsibilities

**lattice-gateway** — Server binary entry point. Loads config, initializes all subsystems, starts the axum server. No business logic.

**lattice-cli** — CLI binary. Commands:
- `lattice compose` — compose subgraph schemas into supergraph SDL (delegates to `@theguild/federation-composition` via subprocess)
- `lattice serve` — start the gateway server
- `lattice check` — validate config and supergraph schema

**lattice-server** — Core request pipeline. Receives GraphQL requests via axum, runs them through the middleware chain (auth, rate limiting, plugins), delegates to federation/execution, returns responses. Owns the lifecycle hook system.

**lattice-federation** — Thin wrapper around `hive-router-query-planner`. Loads supergraph SDL, provides query planning interface. Abstraction boundary: if the upstream planner changes API, only this crate changes.

**lattice-execution** — Thin wrapper around `hive-router-plan-executor`. Executes query plans by dispatching HTTP requests to subgraphs, merges responses. Handles subgraph error propagation.

**lattice-auth** — Built-in JWT validation using `jsonwebtoken`. Enforces federation directives:
- `@authenticated` — requires valid JWT
- `@authorized(policy: "...")` — evaluates policy against JWT claims
- Propagates auth context (claims, headers) to subgraphs
- Implements the `AuthProvider` trait so WASI plugins can replace it entirely.

**lattice-cache** — Entity caching backed by Valkey/Redis. Caches resolved entities by `@key` fields with configurable TTL. Trait-based: `EntityCache` trait with `ValkeyEntityCache` as the default implementation.

**lattice-rate-limit** — Rate limiting backed by Valkey/Redis. Averaging fixed-window algorithm (same approach as the existing implementation — dual-window, lock-free, single roundtrip). Trait-based: `RateLimiter` trait with `ValkeyRateLimiter` as default.

**lattice-plugin** — WASI plugin runtime using `wasmtime`. Loads `.wasm` modules, provides host functions for:
- Reading/modifying request headers
- Reading/modifying GraphQL operation
- Accessing auth context
- Returning early (reject request)
- Logging/metrics

Plugins declare which lifecycle hooks they handle.

**lattice-config** — Parses `lattice.yaml` into typed config. Supports environment variable expansion (`${VAR}`). Validates config at startup.

**lattice-telemetry** — Sets up `tracing` subscriber with OpenTelemetry export. Structured logging. Request-scoped spans with trace ID propagation to subgraphs.

## Request Lifecycle

```
1. HTTP Request (axum)
   │
2. Parse GraphQL operation
   │
3. ──► WASI Hook: on_request(request) ◄── can reject/modify
   │
4. Authenticate (JWT validation)
   │    ├── Built-in: validate JWT, extract claims
   │    └── Plugin: WASI auth provider (if configured, replaces built-in)
   │
5. ──► WASI Hook: on_auth(context) ◄── can enrich auth context
   │
6. Rate limit check
   │
7. Plan query (hive-router-query-planner)
   │    └── Decomposes operation into subgraph fetches
   │
8. ──► WASI Hook: on_plan(plan) ◄── can inspect/modify plan
   │
9. Execute plan (hive-router-plan-executor)
   │    ├── Parallel subgraph HTTP requests
   │    ├── Entity cache lookup/store
   │    └── Response merging
   │
10. ──► WASI Hook: on_response(response) ◄── can modify response
    │
11. Return GraphQL response
```

## WASI Plugin System

Plugins are WebAssembly modules built with **Extism** (BSD-3-Clause), consistent with Vortex and ODK across the Omni ecosystem. Extism provides multi-language PDKs (Rust, Go, JS, Python, etc.) so plugin authors aren't locked to Rust, and handles memory management, serialization, and host function linking automatically.

Plugins compile to `wasm32-unknown-unknown` and run in sandboxed Extism instances with configurable memory and CPU limits. No filesystem or network access unless explicitly granted via host functions.

### Plugin Interface

Plugin functions receive and return JSON-encoded data via Extism's shared memory kernel.

**Exported functions (plugin → host):**

| Function | Input | Output | Description |
|---|---|---|---|
| `on_request` | `GraphqlRequest` | `HookResult` | Inspect/modify/reject incoming request |
| `on_auth` | `AuthContext` | `HookResult` | Enrich or override auth context |
| `on_response` | `GraphqlResponse` | `HookResult` | Modify outgoing response |

**Host functions (host → plugin, `lattice` namespace):**

| Function | Description |
|---|---|
| `lattice_log` | Structured logging |
| `lattice_config_get` | Read plugin config value |
| `lattice_cache_get` / `lattice_cache_set` | Access entity cache |
| `lattice_http` | Outbound HTTP (host-controlled) |

**Example plugin (Rust PDK):**

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct GraphqlRequest {
    query: String,
    headers: Vec<(String, String)>,
}

#[derive(Serialize)]
struct HookResult {
    action: String, // "continue", "modify", "reject"
    data: Option<String>,
}

#[plugin_fn]
pub fn on_request(Json(req): Json<GraphqlRequest>) -> FnResult<Json<HookResult>> {
    // Example: reject introspection queries in production
    if req.query.contains("__schema") {
        return Ok(Json(HookResult {
            action: "reject".into(),
            data: Some(r#"{"errors":[{"message":"introspection disabled"}]}"#.into()),
        }));
    }
    Ok(Json(HookResult { action: "continue".into(), data: None }))
}
```

### Plugin Config

```yaml
# lattice.yaml
plugins:
  - name: custom-auth
    path: ./plugins/custom-auth.wasm
    hooks: [on_request, on_auth]
    config:
      issuer: "https://auth.example.com"

  - name: audit-log
    path: ./plugins/audit-log.wasm
    hooks: [on_response]
```

## Configuration

```yaml
# lattice.yaml

server:
  listen: "0.0.0.0:4000"
  workers: auto  # defaults to CPU count

supergraph:
  path: ./supergraph.graphql
  # OR fetch from URL/registry
  # url: https://registry.example.com/supergraph.graphql
  # poll_interval: 30s

subgraphs:
  # Used by `lattice compose` to generate supergraph.graphql
  aether:
    url: http://aether:3000/graphql
    schema: ./subgraphs/aether.graphql  # optional, for file-based composition

  gatekeeper:
    url: http://gatekeeper:3000/graphql

  warden:
    url: http://warden:3000/graphql

auth:
  jwt:
    issuer: "https://auth.omni.dev"
    audience: "omni"
    jwks_url: "https://auth.omni.dev/.well-known/jwks.json"
    # Header forwarding to subgraphs
    forward_claims: true
    header_name: "Authorization"

cache:
  entity:
    enabled: true
    valkey_url: "${VALKEY_URL}"
    default_ttl: 300  # seconds
    key_prefix: "lattice:"

rate_limit:
  enabled: true
  valkey_url: "${VALKEY_URL}"
  default:
    requests: 1000
    window: 60s
  # Per-operation overrides
  overrides:
    - operations: ["mutation.*"]
      requests: 100
      window: 60s

plugins:
  - name: audit-log
    path: ./plugins/audit-log.wasm
    hooks: [on_response]

telemetry:
  service_name: lattice-gateway
  otlp_endpoint: "${OTEL_EXPORTER_OTLP_ENDPOINT}"
  log_level: info
  log_format: json  # or "pretty" for development
```

## Data Source Connectors

Lattice federates **any standard GraphQL subgraph** — it doesn't care how the subgraph is built. For Postgres-backed services, users can choose their own subgraph implementation:

| Option | License | Effort | Notes |
|---|---|---|---|
| PostGraphile v5 | MIT | Medium | Best FOSS option, federation plugin needs community work |
| Hasura v2 CE | Apache-2.0 (CLA) | Low | Auto-generates from DB, experimental federation v1 |
| Custom service | Your choice | High | Full control, any language/framework |
| Lattice connector (future) | MIT | Zero (for users) | Direct Postgres-to-federation, no sidecar |

### Lattice Connectors (Future)

Connectors are **Extism plugins** that let the gateway connect directly to data sources without a separate subgraph service. They are a differentiator — zero-sidecar federation — but not required for initial release.

Each connector has two components:

1. **CLI plugin** (build time): introspects the data source, generates subgraph SDL
2. **WASM module** (runtime): translates GraphQL operations into native queries

**Connector plugin interface:**

| Function | Phase | Description |
|---|---|---|
| `introspect` | Build time | Receives config JSON, returns subgraph SDL |
| `resolve` | Runtime | Receives field/entity resolution request, returns JSON data |
| `resolve_entities` | Runtime | Batch entity resolution for `@key` lookups |

**Planned first-party connectors:**

| Connector | Data Source | Build Step | Runtime |
|---|---|---|---|
| `postgres` | PostgreSQL | Introspects `pg_catalog`, generates SDL with `@key` from PKs | Translates GraphQL → parameterized SQL |
| `openapi` | REST APIs | Parses OpenAPI spec, generates SDL | Translates GraphQL → HTTP requests |
| `graphql` | GraphQL APIs | Fetches schema via introspection | Proxies operations to upstream |

Community connectors (gRPC, MongoDB, etc.) implement the same Extism interface.

### Connector Config

```yaml
# lattice.yaml
subgraphs:
  # Standard subgraph (external service — any implementation)
  gatekeeper:
    url: http://gatekeeper:3000/graphql

  # Connector-based subgraph (direct data source, no sidecar)
  billing:
    connector:
      type: postgres
      path: ./connectors/postgres.wasm
      config:
        url: "${BILLING_DATABASE_URL}"
        schema: public
        include: [plans, subscriptions, invoices]

  # REST API connector
  payments:
    connector:
      type: openapi
      path: ./connectors/openapi.wasm
      config:
        spec: ./specs/payments-api.yaml
        base_url: "https://api.stripe.com"
        headers:
          Authorization: "Bearer ${STRIPE_SECRET_KEY}"
```

## GraphQL IDE

Lattice embeds **GraphiQL** (MIT, maintained by the GraphQL Foundation) as a built-in development playground.

- Served at `GET /` when `server.playground: true` (enabled by default in dev, disabled in production)
- Single static HTML page bundled into the binary — no external CDN dependencies
- Auto-configured with the gateway's endpoint and available headers
- Supports subscriptions via WebSocket in the IDE

```yaml
# lattice.yaml
server:
  listen: "0.0.0.0:4000"
  playground: true  # default: true when LATTICE_ENV != "production"
```

## Subscriptions

WebSocket transport using the `graphql-ws` protocol (not the legacy `subscriptions-transport-ws`).

The gateway receives subscription requests over WebSocket, plans the subscription query to identify the target subgraph, and establishes a WebSocket connection to that subgraph. Messages from the subgraph are forwarded to the client.

For subscriptions spanning multiple subgraphs: the initial subscription targets one subgraph, and any `@requires` fields from other subgraphs are resolved on each event via standard query execution.

## Key Dependencies

All MIT or Apache-2.0:

| Crate | Purpose | License |
|---|---|---|
| `hive-router-query-planner` | Federation query planning | MIT |
| `hive-router-plan-executor` | Plan execution | MIT |
| `axum` | HTTP server | MIT |
| `tokio` | Async runtime | MIT |
| `extism` | WASI plugin runtime | BSD-3-Clause |
| `jsonwebtoken` | JWT validation | MIT |
| `deadpool-redis` | Valkey/Redis connection pool | MIT |
| `serde` / `serde_yaml` | Config parsing | MIT/Apache-2.0 |
| `tracing` | Structured logging | MIT |
| `opentelemetry` | Telemetry export | Apache-2.0 |
| `clap` | CLI argument parsing | MIT/Apache-2.0 |

**Composition (build-time only, not in Rust binary):**

| Package | Purpose | License |
|---|---|---|
| `@theguild/federation-composition` | Subgraph → supergraph composition | MIT |

## Deployment

```dockerfile
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p lattice-gateway

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/lattice-gateway /usr/local/bin/
EXPOSE 4000
ENTRYPOINT ["lattice-gateway", "--config", "/etc/lattice/lattice.yaml"]
```

Container image published to `ghcr.io/omnidotdev/lattice-gateway`.

## Implementation Order

1. **Workspace scaffold** — Cargo workspace, all crate stubs, CI, cargo-husky
2. **Config** — `lattice-config` crate, YAML parsing with env var expansion
3. **Federation** — `lattice-federation` crate, load supergraph SDL, expose query planning
4. **Server** — `lattice-server` + `lattice-gateway`, axum server accepting GraphQL, wired to planner
5. **Execution** — `lattice-execution` crate, dispatch to subgraphs, merge responses
6. **CLI** — `lattice-cli` with `compose`, `serve`, `check` commands
7. **Auth** — `lattice-auth` crate, JWT validation, directive enforcement
8. **Caching** — `lattice-cache` crate, Valkey entity cache
9. **Rate limiting** — `lattice-rate-limit` crate, Valkey rate limiter
10. **Telemetry** — `lattice-telemetry` crate, tracing + OTel
11. **WASI plugins** — `lattice-plugin` crate, wasmtime runtime, lifecycle hooks
12. **Connectors** — Postgres, OpenAPI, GraphQL connectors as WASI modules
13. **Subscriptions** — WebSocket transport, subscription routing
14. **GraphiQL** — embedded playground at `GET /`

## What This Replaces

This design fully replaces the previous approach of forking and rebranding an upstream project. The previous codebase and its workspace manifest (31 phantom crates) will be removed entirely. The Valkey integration patterns (connection pooling, entity caching, rate limiting algorithms) are re-implemented from the same public domain algorithms, not copied from any copyleft source.

## License

MIT. All dependencies are MIT or Apache-2.0. No copyleft code in the dependency tree.
