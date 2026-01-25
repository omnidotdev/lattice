# Lattice Phase 1 MVP Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Get Lattice running as a federation gateway composing two Omni services (Aether + Warden).

**Architecture:** Fork Grafbase, rebrand to Lattice/Omni, fix GCC 15 build issue, publish container image, configure basic federation of two existing Postgraphile subgraphs.

**Tech Stack:** Rust, Docker, GraphQL Federation v2, Postgraphile

---

## Task 1: Fix GCC 15 Build Compatibility

**Files:**
- Modify: `cli/crates/cli/Cargo.toml:39`

**Step 1: Update syntect to use pure-Rust regex**

Change line 39 from:
```toml
syntect = "5.2.0"
```

To:
```toml
syntect = { version = "5.2.0", default-features = false, features = ["default-syntaxes", "default-themes", "regex-fancy", "parsing", "dump-load", "dump-create", "html", "yaml-load"] }
```

**Step 2: Verify build passes**

Run:
```bash
cargo build --workspace
```
Expected: `Finished` with no errors

**Step 3: Run lib tests**

Run:
```bash
cargo test --workspace --lib --exclude wasi-component-loader --exclude federation-audit-tests
```
Expected: All tests pass

**Step 4: Commit**

```bash
git add cli/crates/cli/Cargo.toml
git commit -m "fix: use pure-Rust regex for GCC 15 compatibility

Switch syntect from onig (C library) to fancy-regex (pure Rust).
Fixes build failures on GCC 15 due to C23 strictness."
```

---

## Task 2: Rebrand Package Names

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `cli/crates/cli/Cargo.toml`
- Modify: `gateway/crates/gateway-binary/Cargo.toml`

**Step 1: Update workspace metadata**

In root `Cargo.toml`, find the `[workspace.package]` section and update:

```toml
[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MPL-2.0"
homepage = "https://lattice.omni.dev"
repository = "https://github.com/omnidotdev/lattice"
```

**Step 2: Rename CLI binary**

In `cli/crates/cli/Cargo.toml`, change:

```toml
[package]
name = "lattice-cli"
```

And the binary name:
```toml
[[bin]]
name = "lattice"
path = "src/main.rs"
```

**Step 3: Rename gateway binary**

In `gateway/crates/gateway-binary/Cargo.toml`, change:

```toml
[package]
name = "lattice-gateway"

[[bin]]
name = "lattice-gateway"
path = "src/main.rs"
```

**Step 4: Verify build**

Run:
```bash
cargo build -p lattice-cli -p lattice-gateway
```
Expected: Binaries built at `target/debug/lattice` and `target/debug/lattice-gateway`

**Step 5: Commit**

```bash
git add Cargo.toml cli/crates/cli/Cargo.toml gateway/crates/gateway-binary/Cargo.toml
git commit -m "chore: rebrand to Lattice/Omni

Rename packages and binaries from grafbase to lattice.
Update repository URLs to omnidotdev."
```

---

## Task 3: Update CLI Branding

**Files:**
- Modify: `cli/crates/cli/src/main.rs`
- Modify: `cli/crates/cli/src/cli.rs` (if exists)

**Step 1: Find and update CLI name/description**

Search for "grafbase" in CLI source:
```bash
grep -rn "grafbase\|Grafbase" cli/crates/cli/src/
```

Update all references to use "lattice" / "Lattice".

**Step 2: Update help text**

Look for clap attributes like:
```rust
#[command(name = "grafbase")]
```

Change to:
```rust
#[command(name = "lattice")]
```

**Step 3: Verify CLI runs**

Run:
```bash
./target/debug/lattice --help
```
Expected: Help text shows "lattice" not "grafbase"

**Step 4: Commit**

```bash
git add cli/
git commit -m "chore: update CLI branding to Lattice"
```

---

## Task 4: Update Gateway Branding

**Files:**
- Modify: `gateway/crates/gateway-binary/src/main.rs`
- Modify: `gateway/crates/federated-server/src/lib.rs`

**Step 1: Find and update gateway references**

Search for "grafbase" in gateway source:
```bash
grep -rn "grafbase\|Grafbase" gateway/
```

Update user-facing strings (logs, error messages) to use "Lattice".

**Step 2: Verify gateway runs**

Run:
```bash
./target/debug/lattice-gateway --help
```
Expected: Help text shows "Lattice" not "Grafbase"

**Step 3: Commit**

```bash
git add gateway/
git commit -m "chore: update gateway branding to Lattice"
```

---

## Task 5: Create Dockerfile

**Files:**
- Create: `Dockerfile`
- Create: `.dockerignore`

**Step 1: Create .dockerignore**

```dockerfile
target/
.git/
*.md
.env*
.cargo/
docs/
examples/
```

**Step 2: Create multi-stage Dockerfile**

```dockerfile
# Build stage
FROM rust:1.82-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace
COPY . .

# Build release binaries
RUN cargo build --release -p lattice-gateway

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/lattice-gateway /usr/local/bin/

EXPOSE 4000

ENTRYPOINT ["lattice-gateway"]
CMD ["--listen-address", "0.0.0.0:4000"]
```

**Step 3: Build Docker image**

Run:
```bash
docker build -t lattice:dev .
```
Expected: Image builds successfully

**Step 4: Test image runs**

Run:
```bash
docker run --rm lattice:dev --help
```
Expected: Help text displayed

**Step 5: Commit**

```bash
git add Dockerfile .dockerignore
git commit -m "build: add Dockerfile for Lattice gateway"
```

---

## Task 6: Create Example Federation Config

**Files:**
- Create: `examples/omni-federation/lattice.config.ts`
- Create: `examples/omni-federation/README.md`

**Step 1: Create config directory**

```bash
mkdir -p examples/omni-federation
```

**Step 2: Create federation config**

`examples/omni-federation/lattice.config.ts`:
```typescript
import { config, graph } from '@grafbase/sdk'

export default config({
  graph: graph.Federated({
    subgraphs: [
      {
        name: 'aether',
        url: process.env.AETHER_GRAPHQL_URL || 'http://localhost:4001/graphql',
      },
      {
        name: 'warden',
        url: process.env.WARDEN_GRAPHQL_URL || 'http://localhost:4002/graphql',
      },
    ],
  }),
})
```

**Step 3: Create README**

`examples/omni-federation/README.md`:
```markdown
# Omni Federation Example

Basic Lattice configuration federating Aether (billing) and Warden (authz) services.

## Prerequisites

- Aether running at `AETHER_GRAPHQL_URL`
- Warden running at `WARDEN_GRAPHQL_URL`

## Run

```bash
export AETHER_GRAPHQL_URL=http://localhost:4001/graphql
export WARDEN_GRAPHQL_URL=http://localhost:4002/graphql

lattice-gateway --config lattice.config.ts
```
```

**Step 4: Commit**

```bash
git add examples/omni-federation/
git commit -m "docs: add Omni federation example config"
```

---

## Task 7: Update Root README

**Files:**
- Modify: `README.md`

**Step 1: Update README header**

Replace the existing README header with:

```markdown
# Lattice

Omni's GraphQL federation gateway - a fork of [Grafbase](https://github.com/grafbase/grafbase) preserving standalone graph capabilities, TypeScript resolvers, and native data source connectors.

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

- [Architecture Design](/docs/plans/2026-01-25-lattice-architecture-design.md)
- [Omni Federation Example](/examples/omni-federation/)

## License

MPL-2.0 (inherited from Grafbase)
```

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: update README for Lattice"
```

---

## Task 8: Create GitHub Actions Workflow

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`

**Step 1: Create CI workflow**

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [master, main]
  pull_request:
    branches: [master, main]

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Build
        run: cargo build --workspace

      - name: Test
        run: cargo test --workspace --lib --exclude wasi-component-loader --exclude federation-audit-tests

  docker:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build Docker image
        run: docker build -t lattice:ci .
```

**Step 2: Create release workflow**

`.github/workflows/release.yml`:
```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository }}

jobs:
  docker:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Log in to Container registry
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}

      - name: Build and push
        uses: docker/build-push-action@v5
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
```

**Step 3: Commit**

```bash
git add .github/
git commit -m "ci: add GitHub Actions workflows for CI and release"
```

---

## Task 9: Add Cargo Config for Build

**Files:**
- Create: `.cargo/config.toml`

**Step 1: Create cargo config**

`.cargo/config.toml`:
```toml
[env]
CC = "/usr/bin/cc"
CFLAGS = "-Wno-incompatible-pointer-types -Wno-int-conversion"

[build]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[target.x86_64-unknown-linux-gnu]
linker = "clang"
```

**Step 2: Add to gitignore exception**

The `.cargo/config.toml` should be tracked. Verify it's not in `.gitignore`.

**Step 3: Commit**

```bash
git add .cargo/config.toml
git commit -m "build: add cargo config for consistent builds"
```

---

## Task 10: Final Integration Test

**Step 1: Clean build**

```bash
cargo clean
cargo build --release -p lattice-gateway
```

**Step 2: Run tests**

```bash
cargo test --workspace --lib --exclude wasi-component-loader --exclude federation-audit-tests
```

**Step 3: Build Docker image**

```bash
docker build -t ghcr.io/omnidotdev/lattice:phase1 .
```

**Step 4: Test Docker image**

```bash
docker run --rm ghcr.io/omnidotdev/lattice:phase1 --help
```

**Step 5: Create phase 1 tag**

```bash
git tag -a v0.1.0-phase1 -m "Phase 1 MVP: Lattice rebranded and containerized"
```

---

## Checklist

- [ ] Task 1: Fix GCC 15 build compatibility
- [ ] Task 2: Rebrand package names
- [ ] Task 3: Update CLI branding
- [ ] Task 4: Update gateway branding
- [ ] Task 5: Create Dockerfile
- [ ] Task 6: Create example federation config
- [ ] Task 7: Update root README
- [ ] Task 8: Create GitHub Actions workflows
- [ ] Task 9: Add cargo config for build
- [ ] Task 10: Final integration test

---

## Next Phase

After Phase 1, proceed to Phase 2: gRPC Connector implementation.

See: `/docs/plans/2026-01-25-lattice-architecture-design.md`
