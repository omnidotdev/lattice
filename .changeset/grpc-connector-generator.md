---
"@omnidotdev/lattice": minor
---

Add `lattice grpc`, which generates a federated subgraph from a protobuf descriptor set. It turns a compiled `FileDescriptorSet` (from `protoc --descriptor_set_out`) into a GraphQL SDL and a `grpc` connector config block: each service's unary methods become root fields (idempotent methods become queries, the rest mutations), request and response messages become input and output types, and enums, nested messages, repeated fields, and 64-bit integers are mapped through the proto3 JSON conventions. Streaming methods are skipped. The generated SDL composes into the supergraph today; runtime gRPC execution follows in a subsequent release.
