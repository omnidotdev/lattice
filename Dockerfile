FROM rust:1.87-bookworm AS builder

WORKDIR /app

# Copy workspace manifests first for layer caching.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/lattice-auth/Cargo.toml crates/lattice-auth/Cargo.toml
COPY crates/lattice-cache/Cargo.toml crates/lattice-cache/Cargo.toml
COPY crates/lattice-cli/Cargo.toml crates/lattice-cli/Cargo.toml
COPY crates/lattice-config/Cargo.toml crates/lattice-config/Cargo.toml
COPY crates/lattice-execution/Cargo.toml crates/lattice-execution/Cargo.toml
COPY crates/lattice-federation/Cargo.toml crates/lattice-federation/Cargo.toml
COPY crates/lattice-plugin/Cargo.toml crates/lattice-plugin/Cargo.toml
COPY crates/lattice-rate-limit/Cargo.toml crates/lattice-rate-limit/Cargo.toml
COPY crates/lattice-server/Cargo.toml crates/lattice-server/Cargo.toml
COPY crates/lattice-telemetry/Cargo.toml crates/lattice-telemetry/Cargo.toml

# Create stub source files so cargo can resolve the workspace.
RUN for crate in lattice-auth lattice-cache lattice-cli lattice-config \
        lattice-execution lattice-federation lattice-plugin \
        lattice-rate-limit lattice-server lattice-telemetry; do \
        mkdir -p "crates/$crate/src" && \
        echo "" > "crates/$crate/src/lib.rs"; \
    done && \
    echo "fn main() {}" > crates/lattice-cli/src/main.rs

# Build dependencies only (cached unless Cargo.toml/lock changes).
RUN cargo build --release -p lattice-cli 2>/dev/null || true

# Copy actual source code.
COPY crates crates
COPY scripts scripts

# Touch source files to invalidate the dependency-only build.
RUN find crates -name "*.rs" -exec touch {} +

# Build the release binary.
RUN cargo build --release -p lattice-cli

# Runtime image.
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/lattice /usr/local/bin/
COPY --from=builder /app/scripts /usr/local/share/lattice/scripts

EXPOSE 4000

ENTRYPOINT ["lattice", "serve"]
CMD ["--config", "/etc/lattice/lattice.yaml"]
