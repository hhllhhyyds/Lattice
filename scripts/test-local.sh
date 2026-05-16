#!/usr/bin/env bash
# Run all tests including real-LLM integration tests.
# Each provider's real-agent run is gated on its API base env var.
set -euo pipefail
cd "$(dirname "$0")/.."

# Load .env so we can branch on provider availability.
if [ -f .env ]; then
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
fi

# Run a real-agent invocation, surfacing full output if it fails so the
# pipe-through-grep does not silently swallow real errors.
run_real_agent() {
  local provider=$1
  local prompt=$2
  local tmp
  tmp=$(mktemp)
  if LATTICE_LLM_PROVIDER="$provider" cargo run --all-features -p real-agent \
      -- "$prompt" >"$tmp" 2>&1; then
    grep -E "Agent Answer|FinalAnswer|^error" "$tmp" || true
    rm -f "$tmp"
  else
    echo "real-agent ($provider) FAILED for prompt: $prompt" >&2
    echo "----- full output -----" >&2
    cat "$tmp" >&2
    rm -f "$tmp"
    return 1
  fi
}

echo "Running unit tests..."
cargo test --workspace --all-features

echo ""
echo "Running integration tests (real LLM, ignored by default)..."
echo "  Covers: e2e (openai + anthropic), llm-openai integration"
cargo test --workspace --all-features -- --ignored

if [ -n "${LATTICE_OPENAI_API_BASE:-}" ]; then
  echo ""
  echo "Running real-agent (openai)..."
  run_real_agent openai "What is 2 + 2? Reply with just the number."
  echo ""
  echo "Running real-agent (openai)..."
  run_real_agent openai "列出本机磁盘空间."
else
  echo ""
  echo "Skipping real-agent (openai): LATTICE_OPENAI_API_BASE not set"
fi

if [ -n "${LATTICE_ANTHROPIC_API_BASE:-}" ]; then
  echo ""
  echo "Running real-agent (anthropic)..."
  run_real_agent anthropic "What is 2 + 2? Reply with just the number."
  echo ""
  echo "Running real-agent (anthropic)..."
  run_real_agent anthropic "列出本机磁盘空间."
else
  echo ""
  echo "Skipping real-agent (anthropic): LATTICE_ANTHROPIC_API_BASE not set"
fi

echo ""
echo "All tests passed!"
