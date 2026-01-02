#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
#
# Environment setup script for Aetherless benchmarks
# Tested on Ubuntu 22.04 LTS

set -euo pipefail

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║         AETHERLESS ENVIRONMENT SETUP                         ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# Check Ubuntu version
if [ -f /etc/os-release ]; then
    . /etc/os-release
    echo "Detected OS: $NAME $VERSION_ID"
else
    echo "Warning: Cannot detect OS version"
fi

# Install Rust if not present
if ! command -v cargo &> /dev/null; then
    echo ""
    echo "▶ Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "✓ Rust already installed: $(rustc --version)"
fi

# Install system dependencies
echo ""
echo "▶ Installing system dependencies..."
if command -v apt-get &> /dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq \
        build-essential \
        pkg-config \
        libssl-dev \
        criu \
        python3 \
        python3-pip \
        || true
elif command -v dnf &> /dev/null; then
    sudo dnf install -y \
        gcc \
        pkg-config \
        openssl-devel \
        criu \
        python3 \
        python3-pip \
        || true
else
    echo "Warning: Package manager not detected, skipping system dependencies"
fi

# Build the project
echo ""
echo "▶ Building Aetherless..."
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo build --release

# Verify build
if [ -f "target/release/aether" ]; then
    echo "✓ CLI built: target/release/aether"
else
    echo "✓ Build completed"
fi

# Check CRIU availability
echo ""
if command -v criu &> /dev/null; then
    CRIU_VERSION=$(criu --version 2>/dev/null | head -1 || echo "unknown")
    echo "✓ CRIU available: $CRIU_VERSION"
else
    echo "⚠ CRIU not installed (optional for warm pools)"
fi

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                 SETUP COMPLETE                                ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "Next steps:"
echo "  ./bench/compare.sh --smoke      # Quick smoke test"
echo "  ./bench/compare.sh --full       # Full benchmark suite"
