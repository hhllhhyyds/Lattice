#!/usr/bin/env pwsh
# Windows equivalent of check.sh

Write-Host "Running cargo fmt..." -ForegroundColor Cyan
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo clippy..." -ForegroundColor Cyan
cargo clippy --workspace --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo test..." -ForegroundColor Cyan
cargo test --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo doc..." -ForegroundColor Cyan
cargo doc --workspace --no-deps --document-private-items
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "All checks passed!" -ForegroundColor Green
