---
"@omnidotdev/lattice": minor
---

Generate a REST subgraph directly from an OpenAPI spec. `lattice openapi <spec>` reads an OpenAPI 3.x document (JSON or YAML) and emits a GraphQL SDL plus a ready-to-paste `lattice.yaml` connector block: each operation becomes a Query or Mutation field, path and query parameters become arguments, request bodies become input types, and referenced schemas become GraphQL types. No schema or field mappings need to be hand-written.
