# 全lintチェックを実行するスクリプト
# pre-pushフック および手動実行用
$ErrorActionPreference = "Stop"

Write-Host "=== cargo fmt --check ==="
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== cargo clippy ==="
cargo clippy -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== cargo test ==="
cargo test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# node_modules がある場合はフロントエンド系lintも実行
if ((Test-Path "node_modules") -and (Test-Path "package.json"))
{
    Write-Host "=== markdownlint ==="
    pnpm run markdownlint 2>&1 | Out-Null
    Write-Host "=== textlint ==="
    pnpm run textlint 2>&1 | Out-Null
    Write-Host "=== prettier:check ==="
    pnpm run prettier:check 2>&1 | Out-Null
}

Write-Host "All checks passed."
