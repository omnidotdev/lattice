# Lattice Compose CLI Design

**Goal:** Add a `lattice compose` command to compose subgraph schemas into a federated supergraph SDL.

---

## Command Interface

```bash
# Basic usage - output to stdout
lattice compose --config supergraph.yaml

# Output to file
lattice compose --config supergraph.yaml --output supergraph.graphql
```

## Config File Format

```yaml
# supergraph.yaml

subgraphs:
  # Introspection mode - fetches schema at compose time
  aether:
    url: http://localhost:4001/graphql

  # File mode - uses local SDL file
  warden:
    schema: ./schemas/warden.graphql
    routing_url: http://warden:4002/graphql

  # File mode with introspection fallback
  inventory:
    schema: ./schemas/inventory.graphql
    url: http://localhost:4003/graphql

# Optional: headers for introspection requests
headers:
  Authorization: "Bearer ${SUBGRAPH_TOKEN}"
  X-Custom-Header: "static-value"
```

**Validation rules:**
- Each subgraph needs either `url` or `schema` (or both)
- If only `schema` is provided, `routing_url` is required
- Environment variables use `${VAR}` syntax, error if unset

---

## Implementation

### New Files

```
cli/crates/cli/src/
├── compose.rs              # Main compose command logic
├── compose/
│   └── config.rs           # Config file parsing
└── cli_input/
    └── compose.rs          # Clap args
```

### Dependencies

All already in workspace:
- `graphql-composition` - Core composition logic
- `grafbase-graphql-introspection` - Schema fetching
- `serde` + `serde_yaml` - Config parsing

### Flow

1. Parse CLI args (`--config`, `--output`)
2. Load and parse `supergraph.yaml`
3. For each subgraph:
   - If `schema` field exists: read file
   - Else: introspect from `url`
4. Call `graphql_composition::compose()`
5. If errors: print diagnostics, exit 1
6. If warnings: print to stderr
7. Output SDL to stdout or `--output` file

---

## Error Handling

**Composition errors** (fatal, exit 1):
```
Error: Composition failed

  × Field 'User.id' has incompatible types across subgraphs
  │ aether: ID!
  │ warden: String!
  ╰─▶ Fields with @key must have identical types
```

**Validation warnings** (non-fatal, stderr):
```
Warning: Field 'User.email' is deprecated without reason

Composed 2 subgraphs into supergraph.graphql
```

**Exit codes:**
- `0` - Success (may have warnings)
- `1` - Composition failed
- `2` - Config/file errors

---

## Output

- SDL goes to stdout by default (pipe-friendly)
- With `--output`, writes to file
- Summary/warnings go to stderr (doesn't pollute piped output)
