"""pyfltr の jsonl 出力を読み、失敗コマンドを GitHub Actions の ::error:: アノテーションに変換する。

CI で `mise run ci` (= `uvx pyfltr ci`) が失敗したとき、どの pyfltr コマンドが落ちたかを
ログに入らずとも見えるようにするための診断補助スクリプト。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pyfltr-annotate.py <pyfltr-run.jsonl>", file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    if not path.exists():
        print(f"::error::{path} が見つかりません")
        return 0
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("kind") != "command":
            continue
        status = obj.get("status", "?")
        if status not in ("failed", "resolution_failed"):
            continue
        cmd = obj.get("command", "?")
        msg = (obj.get("message") or "no message").replace("\n", " / ").replace("\r", "")
        # GitHub Actions の ::error:: 1件あたり実用上 4KB まで詰められるため、安全側で 4000 文字に切る。
        if len(msg) > 4000:
            msg = msg[:4000] + "…"
        print(f"::error title=pyfltr {cmd} ({status})::{msg}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
