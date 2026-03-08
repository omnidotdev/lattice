# Build stage
FROM rust:1.94-bookworm AS builder

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
