# Lattice

Omni's GraphQL federation gateway. Originally forked from [the upstream gateway project](https://github.com/grafbase/grafbase).

## Quick Start

```bash
# Build
cargo build --release -p lattice-gateway

# Run with config
./target/release/lattice-gateway --config path/to/lattice.config.ts
```

## Docker

```bash
docker pull ghcr.io/omnidotdev/lattice:latest
docker run -p 4000:4000 ghcr.io/omnidotdev/lattice:latest
```

## Documentation

- [Phase 1 MVP Plan](/docs/plans/2026-01-25-lattice-phase1-mvp.md)
- [Omni Federation Example](/examples/omni-federation/)

## License

MIT
