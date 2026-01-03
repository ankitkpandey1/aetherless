#!/bin/bash
set -e

echo "== Aetherless Benchmarking =="
echo "Building in RELEASE mode..."
cargo build --release

echo "Running benchmarks..."
# Run the benchmark tool (assumes a benchmark binary or test)
# Since we don't have a dedicated separate benchmark suite setup in Cargo.toml yet beyond tests,
# we will use cargo test --release --bench ... if benches exist, or run a stress test script.

if [ -f "target/release/benchmark" ]; then
    ./target/release/benchmark
else
    # Fallback to cargo bench if standard benches exist
    cargo bench
fi

echo "Done."
