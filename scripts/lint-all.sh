#!/usr/bin/env bash
# 全lintチェックを実行するスクリプト
# pre-pushフック および手動実行用
set -euo pipefail

echo "=== cargo fmt --check ==="
cargo fmt --check

echo "=== cargo clippy ==="
cargo clippy -- -D warnings

echo "=== cargo test ==="
cargo test

# node_modules がある場合はフロントエンド系lintも実行
if [ -d "node_modules" ]; then
    if [ -f "package.json" ]; then
        echo "=== markdownlint ==="
        pnpm run markdownlint
        echo "=== textlint ==="
        pnpm run textlint
        echo "=== prettier:check ==="
        pnpm run prettier:check
    fi
fi

echo "All checks passed."
