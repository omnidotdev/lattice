---
"@omnidotdev/lattice": minor
---

Add `response` field remapping to REST connectors. When an upstream's JSON field names differ from the GraphQL schema, map a GraphQL field name to a source key or JSON Pointer in the (post-`select`) response; unmapped keys pass through and arrays are remapped element by element.
