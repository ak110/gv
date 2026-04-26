"""pyfltr の生出力 (output.log) の末尾だけを GitHub Actions の ::error:: アノテーションに変換する。

CI で使うスクリプト。``PYFLTR_CMD`` (ツール名) と ``PYFLTR_LOG`` (ファイルパス) を
環境変数で受け取り、末尾側 6000 文字だけを残して 1 件の ::error:: として出力する。
``pyfltr ci`` の jsonl ``message`` フィールドが既定 2000 文字でハイブリッド中略され、
clippy のようにコンパイルログが先頭を埋めると本体のエラー位置が落ちるため、
末尾側を改めてアノテーション化することでファイル名・行番号を救う。
"""

from __future__ import annotations

import os
import sys
from pathlib import Path


def main() -> int:
    cmd = os.environ.get("PYFLTR_CMD", "?")
    log_path = os.environ.get("PYFLTR_LOG")
    if not log_path:
        print("::error::PYFLTR_LOG が設定されていません", file=sys.stderr)
        return 0
    path = Path(log_path)
    if not path.exists() or path.stat().st_size == 0:
        return 0
    raw = path.read_text(encoding="utf-8", errors="replace")
    tail = raw[-6000:]
    tail = tail.replace("\n", " / ").replace("\r", "")
    if not tail.strip():
        return 0
    print(f"::error title=pyfltr {cmd} (output tail)::{tail}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
