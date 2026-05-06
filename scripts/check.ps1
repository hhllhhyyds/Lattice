#!/usr/bin/env pwsh
# Windows equivalent of check.sh

Write-Host "Running cargo fmt..." -ForegroundColor Cyan
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo clippy..." -ForegroundColor Cyan
cargo clippy --workspace --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo test..." -ForegroundColor Cyan
cargo test --workspace --all-targets --all-features
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue) {
    Write-Host "Running cargo llvm-cov..." -ForegroundColor Cyan
    cargo llvm-cov test --workspace --all-features --lcov --output-path lcov.info -- --nocapture
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Host "Coverage report: lcov.info" -ForegroundColor Green
} else {
    Write-Host "Skipping local coverage validation because cargo-llvm-cov is not installed." -ForegroundColor Yellow
}

Write-Host "Running cargo doc..." -ForegroundColor Cyan
cargo doc --no-deps --all-features
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "All checks passed!" -ForegroundColor Green
