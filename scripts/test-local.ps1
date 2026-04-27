#!/usr/bin/env pwsh
# Windows equivalent of test-local.sh
# Requires .env file with LATTICE_API_KEY

if (-not (Test-Path .env)) {
    Write-Host "Error: .env file not found" -ForegroundColor Red
    Write-Host "Create .env with: LATTICE_API_KEY=your-key" -ForegroundColor Yellow
    exit 1
}

Write-Host "Loading .env..." -ForegroundColor Cyan
Get-Content .env | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        $name = $matches[1]
        $value = $matches[2]
        Set-Item -Path "env:$name" -Value $value
    }
}

Write-Host "Running integration tests with real LLM..." -ForegroundColor Cyan
cargo test --workspace -- --ignored

if ($LASTEXITCODE -eq 0) {
    Write-Host "Integration tests passed!" -ForegroundColor Green
} else {
    Write-Host "Integration tests failed!" -ForegroundColor Red
    exit $LASTEXITCODE
}
