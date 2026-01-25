# Omni Federation Example

Lattice configuration federating Aether (billing) and Warden (authz) services.

## Prerequisites

- Aether running at `AETHER_GRAPHQL_URL`
- Warden running at `WARDEN_GRAPHQL_URL`

## Compose & Run

```bash
# Set subgraph URLs
export AETHER_GRAPHQL_URL=http://localhost:4001/graphql
export WARDEN_GRAPHQL_URL=http://localhost:4002/graphql

# Compose supergraph (pick your preferred format)
lattice compose --config supergraph.yaml --output supergraph.graphql
# or: lattice compose --config supergraph.json --output supergraph.graphql
# or: lattice compose --config supergraph.jsonc --output supergraph.graphql

# Run gateway
lattice-gateway --schema supergraph.graphql --listen-address 0.0.0.0:4000
```

## Config Formats

Lattice supports YAML, JSON, and JSONC (JSON with comments):

**YAML** (`supergraph.yaml`):
```yaml
subgraphs:
  aether:
    url: ${AETHER_GRAPHQL_URL}
  warden:
    url: ${WARDEN_GRAPHQL_URL}
```

**JSON** (`supergraph.json`):
```json
{
  "subgraphs": {
    "aether": { "url": "${AETHER_GRAPHQL_URL}" },
    "warden": { "url": "${WARDEN_GRAPHQL_URL}" }
  }
}
```

**JSONC** (`supergraph.jsonc`):
```jsonc
{
  // Comments are supported
  "subgraphs": {
    "aether": { "url": "${AETHER_GRAPHQL_URL}" },
    "warden": { "url": "${WARDEN_GRAPHQL_URL}" }
  }
}
```

## Using Local Schema Files

If subgraphs aren't running, use local schema files:

```yaml
subgraphs:
  aether:
    schema: ./schemas/aether.graphql
    routing_url: http://aether:4001/graphql
  warden:
    schema: ./schemas/warden.graphql
    routing_url: http://warden:4002/graphql
```

## Docker

```bash
# Build supergraph
lattice compose --config supergraph.yaml --output supergraph.graphql

# Run with Docker
docker run -p 4000:4000 \
  -v $(pwd)/supergraph.graphql:/supergraph.graphql \
  ghcr.io/omnidotdev/lattice:latest \
  --schema /supergraph.graphql \
  --listen-address 0.0.0.0:4000
```
