#!/usr/bin/env bash
# Run all tests including real-LLM integration tests.
# Requires .env file with LATTICE_API_KEY, LATTICE_API_BASE, LATTICE_MODEL.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Running unit tests..."
cargo test --workspace --all-features

echo ""
echo "Running integration tests (real LLM, ignored by default)..."
cargo test --workspace --all-features -- --ignored

echo ""
echo "All tests passed!"
