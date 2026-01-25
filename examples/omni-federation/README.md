# Omni Federation Example

Basic Lattice configuration federating Aether (billing) and Warden (authz) services.

## Prerequisites

- Aether running at `AETHER_GRAPHQL_URL`
- Warden running at `WARDEN_GRAPHQL_URL`

## Run

```bash
export AETHER_GRAPHQL_URL=http://localhost:4001/graphql
export WARDEN_GRAPHQL_URL=http://localhost:4002/graphql

lattice-gateway --config lattice.config.ts
```
