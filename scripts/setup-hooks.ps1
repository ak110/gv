# gitフックのセットアップ
$ErrorActionPreference = "Stop"

Push-Location (git rev-parse --show-toplevel)
git config core.hooksPath .githooks
Write-Host "Git hooks configured: .githooks/"
Pop-Location
