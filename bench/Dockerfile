# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
#
# Dockerfile for reproducible Aetherless benchmarks
# Usage:
#   docker build -t aetherless-bench -f bench/Dockerfile .
#   docker run --rm -v $(pwd)/bench/results:/out aetherless-bench

FROM rust:1.75-bookworm AS builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY aetherless-cli ./aetherless-cli
COPY aetherless-core ./aetherless-core
COPY aetherless-ebpf ./aetherless-ebpf
COPY benchmark ./benchmark
COPY bench ./bench

# Build release
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy built artifacts
COPY --from=builder /app/target/release/aether /usr/local/bin/
COPY --from=builder /app/bench /app/bench

# Make scripts executable
RUN chmod +x /app/bench/*.sh

# Output directory
VOLUME ["/out"]

# Run benchmarks
ENTRYPOINT ["/app/bench/compare.sh", "--smoke", "--output-dir", "/out"]
