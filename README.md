# Lattice

GraphQL federation gateway for the [Omni](https://omni.dev) ecosystem.

## Features

- Federation v2 query planning and execution
- WebSocket subscriptions (`graphql-transport-ws` protocol)
- `@defer` incremental delivery
- JWT authentication with directive enforcement
- Entity and response caching, plus rate limiting (Valkey/Redis)
- Query depth, complexity, and alias limits
- Automatic Persisted Queries (APQ)
- WASI plugin system (Extism)
- Built-in GraphiQL playground
- `/health` liveness endpoint
- Prometheus metrics and OpenTelemetry tracing

## Quick Start

```bash
# Compose subgraph schemas
lattice compose --config lattice.yaml --output supergraph.graphql

# Validate configuration
lattice check --config lattice.yaml

# Start the gateway
lattice serve --config lattice.yaml
```

## Docker

```bash
docker run -v ./lattice.yaml:/etc/lattice/lattice.yaml \
  -v ./supergraph.graphql:/etc/lattice/supergraph.graphql \
  -p 4000:4000 \
  ghcr.io/omnidotdev/lattice
```

## Configuration

```yaml
server:
  listen: "0.0.0.0:4000"
  playground: true

supergraph:
  path: ./supergraph.graphql

subgraphs:
  users:
    url: http://users:4001/graphql
  products:
    url: http://products:4002/graphql
```

See the [configuration reference](https://docs.omni.dev/products/lattice/configuration) for auth, caching, rate limiting, persisted queries, telemetry, and plugin options.

## License

The code in this repository is licensed under Apache 2.0, &copy; [Omni LLC](https://omni.dev). See [LICENSE.md](LICENSE.md) for more information.
