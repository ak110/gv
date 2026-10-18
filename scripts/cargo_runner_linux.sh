#!/usr/bin/env bash
# Linuxからのクロスビルドでcargoがテスト実行器として起動するスクリプト（scripts/setup_linux.pyが設定する）。
# LinuxではWindows向けのテストを実行せず、コンパイルの成功までを確かめる。テストの実行はWindows CIが担う。
set -euo pipefail
echo "Linuxではテストを実行しない（Windows CIで実行する）: ${1:-}"
