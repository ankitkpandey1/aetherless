#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
#
# Build script for Aetherless XDP BPF programs
#
# Usage: ./build_bpf.sh
#
# Requirements:
#   - clang (with BPF target support)
#   - linux-headers or libbpf-dev
#   - bpftool (optional, for BTF generation)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BPF_SRC_DIR="${SCRIPT_DIR}/src/bpf"
BPF_OUT_DIR="${SCRIPT_DIR}/target/bpf"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Check for required tools
check_requirements() {
    if ! command -v clang &> /dev/null; then
        error "clang not found. Install with: sudo apt install clang"
    fi

    # Check clang version (need 10+)
    CLANG_VERSION=$(clang --version | head -n1 | grep -oP '\d+\.\d+\.\d+' | head -1 | cut -d. -f1)
    if [ "$CLANG_VERSION" -lt 10 ]; then
        error "clang version 10+ required, found $CLANG_VERSION"
    fi

    # Check for BPF headers
    if [ ! -d "/usr/include/bpf" ] && [ ! -d "/usr/include/linux" ]; then
        warn "BPF headers not found. Install with: sudo apt install libbpf-dev linux-headers-$(uname -r)"
    fi

    info "Requirements satisfied: clang $CLANG_VERSION"
}

# Compile a single BPF program
compile_bpf() {
    local src_file="$1"
    local out_file="$2"
    
    info "Compiling: $(basename "$src_file") -> $(basename "$out_file")"

    clang \
        -target bpf \
        -O2 \
        -g \
        -Wall \
        -Werror \
        -D__TARGET_ARCH_x86 \
        -I/usr/include \
        -I/usr/include/bpf \
        -c "$src_file" \
        -o "$out_file"

    info "  Size: $(stat -f%z "$out_file" 2>/dev/null || stat -c%s "$out_file") bytes"
}

# Generate BTF skeleton (optional)
generate_skeleton() {
    local obj_file="$1"
    local skel_file="$2"
    
    if command -v bpftool &> /dev/null; then
        info "Generating skeleton: $(basename "$skel_file")"
        bpftool gen skeleton "$obj_file" > "$skel_file" 2>/dev/null || \
            warn "Skeleton generation failed (bpftool might need newer version)"
    fi
}

main() {
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║           AETHERLESS BPF BUILD                               ║"
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo

    check_requirements

    # Create output directory
    mkdir -p "$BPF_OUT_DIR"

    # Compile XDP redirect program
    if [ -f "${BPF_SRC_DIR}/xdp_redirect.c" ]; then
        compile_bpf \
            "${BPF_SRC_DIR}/xdp_redirect.c" \
            "${BPF_OUT_DIR}/xdp_redirect.o"
        
        generate_skeleton \
            "${BPF_OUT_DIR}/xdp_redirect.o" \
            "${BPF_OUT_DIR}/xdp_redirect.skel.h"
    else
        warn "xdp_redirect.c not found"
    fi

    echo
    info "Build complete! Output in: ${BPF_OUT_DIR}"
    echo
    echo "To load the XDP program:"
    echo "  sudo ./target/release/aetherless-ebpf eth0 ${BPF_OUT_DIR}/xdp_redirect.o"
}

main "$@"
