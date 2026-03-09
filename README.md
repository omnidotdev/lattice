# Lattice

GraphQL federation gateway for the [Omni](https://omni.dev) ecosystem.

## Features

- Federation v2 query planning and execution
- JWT authentication with directive enforcement
- Entity caching and rate limiting (Valkey/Redis)
- Extism plugin system (WASI)
- Data source connectors (Postgres, OpenAPI)
- Built-in GraphiQL playground

## Quick Start

```bash
# Compose subgraph schemas
lattice compose --config lattice.yaml --output supergraph.graphql

# Start the gateway
lattice-gateway --config lattice.yaml
```

## License

MIT
