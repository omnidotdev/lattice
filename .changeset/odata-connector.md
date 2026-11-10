---
"@omnidotdev/lattice": minor
---

Add `lattice odata`, which federates an OData service as a subgraph. It turns an OData 4.0 `$metadata` (EDMX) document into a GraphQL SDL and a REST connector config: each entity set becomes a collection query and a by-key query, each entity and complex type becomes a GraphQL type, and enums, collection-valued properties, 64-bit numbers, and composite keys are mapped through. Because an OData service is plain HTTP and JSON, the generated config runs on the existing REST connector unchanged. Navigation properties are skipped (they require `$expand`).
