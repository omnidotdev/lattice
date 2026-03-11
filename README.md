# Lattice

GraphQL federation gateway for the [Omni](https://omni.dev) ecosystem.

## Features

- Federation v2 query planning and execution
- WebSocket subscriptions (`graphql-transport-ws` protocol)
- JWT authentication with directive enforcement
- Entity caching and rate limiting (Valkey/Redis)
- WASI plugin system (Extism)
- Built-in GraphiQL playground
- OpenTelemetry tracing

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
  ghcr.io/omnidotdev/lattice-gateway
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

See the [configuration reference](docs/) for auth, caching, rate limiting, telemetry, and plugin options.

## License

MIT
