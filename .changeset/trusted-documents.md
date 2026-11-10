---
"@omnidotdev/lattice": minor
---

Add trusted documents (persisted operation allowlisting). Set `persisted_queries.require: true` with a `manifest`, and the gateway runs only the operations in that manifest: it preloads them, rejects freeform queries and any operation outside the set, and registers nothing at runtime. The manifest is a JSON object mapping each operation's lowercase hex SHA-256 to its document, and every entry is verified against its hash at load, so a tampered manifest is refused. Trusted mode fails closed: if the manifest cannot be read or parsed, all operations are rejected. Builds on Automatic Persisted Queries, so clients send the operation hash exactly as they would for APQ.
