---
"@omnidotdev/lattice": minor
---

Add a gRPC connector: federate a gRPC service as a subgraph.

`lattice grpc` turns a compiled protobuf `FileDescriptorSet` (from `protoc --descriptor_set_out`) into a GraphQL SDL and a `grpc` connector config block. Each service's unary methods become root fields (idempotent methods queries, the rest mutations), request and response messages become input and output types, and enums, nested messages, repeated fields, and 64-bit integers follow the proto3 JSON conventions. Streaming methods are skipped.

At request time, a `grpc` subgraph resolves over the wire: the gateway encodes each field's input as the request message, makes the unary call, and decodes the response back into GraphQL, using the descriptor set for protobuf coding. Configure a subgraph with a `grpc` block (`endpoint`, `descriptor_set`, `fields`) just like a `rest` one.
