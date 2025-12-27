#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
#
# Run all Aetherless benchmarks and generate reports.
# Usage: ./run_all.sh [--quick]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHMARK_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(dirname "$BENCHMARK_DIR")"
OUTPUT_DIR="$BENCHMARK_DIR/data/$(date +%Y-%m-%d)"

# Parse arguments
QUICK_MODE=""
if [[ "$1" == "--quick" ]]; then
    QUICK_MODE="--quick"
fi

echo "=========================================="
echo "Aetherless Benchmark Suite"
echo "=========================================="
echo "Project root: $PROJECT_ROOT"
echo "Output dir:   $OUTPUT_DIR"
echo ""

mkdir -p "$OUTPUT_DIR"

# Build the benchmark crate
echo "Building benchmark crate..."
cd "$BENCHMARK_DIR"
cargo build --release

# Run the benchmark runner
echo ""
echo "Running benchmarks..."
cargo run --release --bin run_benchmarks -- \
    --output "$OUTPUT_DIR" \
    $QUICK_MODE

# Run criterion benchmarks (optional, more detailed)
if [[ -z "$QUICK_MODE" ]]; then
    echo ""
    echo "Running criterion benchmarks..."
    cargo bench --bench ring_buffer -- --noplot 2>/dev/null || true
fi

# Run baseline comparison scripts
echo ""
echo "Running baseline comparisons..."
if [[ -f "$SCRIPT_DIR/cold_start_baseline.py" ]]; then
    python3 "$SCRIPT_DIR/cold_start_baseline.py" --output "$OUTPUT_DIR" --iterations 20 || true
fi

if [[ -f "$SCRIPT_DIR/http_ipc_baseline.py" ]]; then
    python3 "$SCRIPT_DIR/http_ipc_baseline.py" --output "$OUTPUT_DIR" --iterations 100 || true
fi

echo ""
echo "=========================================="
echo "Benchmarks complete!"
echo "Results saved to: $OUTPUT_DIR"
echo "=========================================="

# List generated files
echo ""
echo "Generated files:"
ls -la "$OUTPUT_DIR"/*.json 2>/dev/null || echo "  (no JSON files generated)"
