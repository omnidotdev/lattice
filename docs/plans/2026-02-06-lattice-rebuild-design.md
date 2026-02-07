# Lattice Rebuild Design

Rebuild lattice as a lean, MIT-licensed GraphQL federation gateway with a plugin-based connector architecture.

## Goals

- Re-license from MPL-2.0 to MIT
- Cut unused/legacy code from the grafbase fork
- Restructure into a clean, obvious layout
- Establish an SDK-based connector model (Rust-first, TypeScript convenience layer)
- Remove all Pathfinder (playground) references

## Architecture

```
┌─────────────────────────────────┐
│         TypeScript SDK          │  Convenience layer (napi-rs or WASM bindings)
├─────────────────────────────────┤
│           Rust SDK              │  Connector traits, plugin interface
├─────────────────────────────────┤
│          Rust Core              │  Federation gateway, composition, query planning
└─────────────────────────────────┘
```

- **Rust core** — gateway runtime, composition engine, query planner
- **Rust SDK** — public trait interface for writing connectors (OpenAPI, Postgres, gRPC, custom)
- **TypeScript SDK** — wraps the Rust SDK for TS developers

Connectors are separate crates built on the SDK, not baked into core.

## Target Structure

```
lattice/
├── core/
│   ├── composition/          # Schema composition algorithm
│   ├── federated-graph/      # Federated graph data structures
│   ├── engine/               # Query planning & execution (current engine-v2)
│   ├── gateway/              # Gateway server runtime
│   ├── common-types/         # Shared type definitions
│   ├── dataloader/           # Batching for subgraph requests
│   ├── telemetry/            # Tracing & observability
│   └── validation/           # Schema validation
├── sdk/
│   └── lattice-sdk/          # Rust SDK — connector traits (new)
├── cli/
│   └── lattice-cli/          # compose, introspect, lint
├── tools/
│   ├── graphql-lint/         # GraphQL linting
│   └── graphql-introspection/  # Introspection utilities
└── examples/
```

## What to Remove

### Entire directories

- `packages/` — client libraries (Apollo, Houdini, URQL), cli-app (Pathfinder wrapper), ESLint config
- `auth-testing/` — test utility
- `workspace-hack/` — build optimization hack

### Legacy engine (v1)

All crates under `engine/crates/engine/`:
- `engine/parser`
- `engine/registry-v1`, `registry-v2`, `registry-v2-generator`
- `engine/registry-upgrade`, `registry-for-cache`
- `engine/response`, `engine/value`, `engine/scalars`
- `engine/derive`, `engine/meta-type-name`
- `engine/query-path`, `engine/validation`
- Top-level `engine/` crate itself

### Connectors (become SDK plugins later)

- `parser-openapi` — OpenAPI/REST connector
- `parser-postgres` — PostgreSQL connector
- `postgres-connector-types` — PostgreSQL types

### CLI crates & commands

- `grafbase-local-backend` — local dev server backend
- `grafbase-local-common` — shared local dev utilities
- Registry-specific CLI commands: publish, check, trust, login, logout, branch, create, subgraphs, upgrade

### Pathfinder (playground)

- `packages/cli-app/` — Pathfinder React wrapper app
- `engine/crates/engine/src/http/pathfinder_source.rs` — HTML template generator
- `engine/crates/engine/src/http/pathfinder.html` — HTML template
- `cli/crates/common/src/pathfinder.rs` — CLI integration
- All references to `assets.grafbase.com` CDN

## What to Keep

### Core crates

- `graphql-composition` — schema composition algorithm
- `composition` — composition orchestration
- `federated-graph` — federated graph data structures
- `engine-v2` + subcrates (schema, query-planning, config, axum, walker, codegen, id-derives, id-newtypes)
- `gateway-core` — core gateway functionality
- `federated-server` — federation server library
- `gateway-config` — gateway configuration
- `common-types` — shared types
- `dataloader` — DataLoader for batching
- `telemetry` — observability
- `validation` — schema validation
- `parser-graphql` — GraphQL schema parser
- `parser-sdl` — SDL parser
- `async-runtime` — async runtime abstractions
- `runtime`, `runtime-local`, `runtime-noop` — runtime implementations
- `jwt-verifier` — JWT verification
- `rolling-logger` — log rotation

### Tools

- `graphql-lint` — GraphQL linting
- `graphql-introspection` — introspection utilities
- `gqlint` — linting CLI
- `graph-ref` — graph reference utilities

### CLI

- `lattice-cli` — compose command (+ introspect, lint)

## Phases

### Phase 1: License swap, remove cut crates, clean workspace

- Replace LICENSE file with MIT
- Update `license` field in all Cargo.toml files to MIT
- Remove all directories/crates listed in "What to Remove"
- Remove Pathfinder references
- Update workspace Cargo.toml members
- Verify `cargo check` passes

### Phase 2: Restructure directory layout

- Move kept crates into `core/`, `cli/`, `tools/` directories
- Update workspace member paths
- Update internal crate references
- Verify `cargo check` passes

### Phase 3: Rebrand remaining grafbase references

- Rename remaining "grafbase" references in code, configs, error messages
- Update crate names where applicable
- Verify `cargo check` and `cargo test` pass

### Phase 4: Design & implement lattice-sdk

- Define `Connector` trait interface
- Establish plugin discovery/loading mechanism
- Create example connector

### Phase 5: TypeScript SDK

- Set up napi-rs or WASM bindings
- Wrap Rust SDK traits for TypeScript consumers
- Publish `@omnidotdev/lattice-sdk` package
