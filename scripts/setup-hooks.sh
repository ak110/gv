#!/usr/bin/env bash
# gitフックのセットアップ
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
git config core.hooksPath .githooks
echo "Git hooks configured: .githooks/"
