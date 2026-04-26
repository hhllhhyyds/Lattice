#!/usr/bin/env bash
# Pre-commit / CI quality checks.
# Run this after any code change to verify everything passes.
# Exit code 0 = all good, non-zero = something failed.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== 1/4 Format check ==="
cargo fmt --all -- --check

echo ""
echo "=== 2/4 Clippy ==="
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo ""
echo "=== 3/4 Tests ==="
cargo test --workspace --all-targets --all-features

echo ""
echo "=== 4/4 Doc check ==="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

echo ""
echo "✅ All checks passed!"
