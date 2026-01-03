#!/bin/bash
set -e

# Master Verification Script
# Runs:
# 1. Formatting Check
# 2. Clippy Linting
# 3. Unit Tests
# 4. E2E Verification
# 5. Autoscaling Verification
# 6. Coverage Report (if available)

GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}== 1. Checking Formatting ==${NC}"
cargo fmt --all -- --check

echo -e "${GREEN}== 2. Running Clippy ==${NC}"
cargo clippy --workspace -- -D warnings

echo -e "${GREEN}== 3. Running Unit Tests ==${NC}"
cargo test --workspace

echo -e "${GREEN}== 4. Running E2E Verification ==${NC}"
./scripts/verify_e2e.sh

echo -e "${GREEN}== 5. Verifying Autoscaling ==${NC}"
if [ -f "./scripts/verify_scaling.sh" ]; then
    ./scripts/verify_scaling.sh || { echo "⚠ Autoscaling verification failed"; exit 1; }
else
    echo "⚠ scripts/verify_scaling.sh not found"
fi

echo -e "${GREEN}== 6. Code Coverage ==${NC}"
if command -v cargo-tarpaulin &> /dev/null; then
    echo "Running tarpaulin..."
    # Exclude E2E test files and main binaries from coverage if needed, but we want generally everything
    cargo tarpaulin --workspace --out Html --output-dir coverage/
    echo "Coverage report generated in coverage/tarpaulin-report.html"
else
    echo "cargo-tarpaulin not found. Skipping coverage report."
    echo "Install with: cargo install cargo-tarpaulin"
fi

echo -e "${GREEN}== All Verification Steps Passed ==${NC}"
