"""pyfltr の jsonl 出力を読み、失敗コマンドを GitHub Actions の ::error:: アノテーションに変換する。

CI で `mise run ci` (= `uvx pyfltr ci`) が失敗したとき、どの pyfltr コマンドが落ちたかを
ログに入らずとも見えるようにするための診断補助スクリプト。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def safe_print(text: str) -> None:
    """Windows runner で stdout が cp932 等の場合でも落ちないように代替コードポイントを使う。"""
    encoding = (sys.stdout.encoding or "utf-8").lower()
    try:
        sys.stdout.write(text + "\n")
        sys.stdout.flush()
    except UnicodeEncodeError:
        encoded = text.encode(encoding, errors="replace").decode(encoding, errors="replace")
        sys.stdout.write(encoded + "\n")
        sys.stdout.flush()


def main() -> int:
    if len(sys.argv) != 2:
        sys.stderr.write("usage: pyfltr-annotate.py <pyfltr-run.jsonl>\n")
        return 0
    path = Path(sys.argv[1])
    if not path.exists():
        safe_print(f"::warning file=.github/workflows/ci.yaml,line=1::{path} not found")
        return 0
    try:
        raw_text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        safe_print(f"::warning file=.github/workflows/ci.yaml,line=1::failed to read {path}: {exc}")
        return 0
    for raw in raw_text.splitlines():
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
        msg_raw = obj.get("message") or "no message"
        msg = msg_raw.replace("\n", " / ").replace("\r", "")
        # GitHub Actions の ::error:: 1件あたり実用上 4KB まで詰められるため、安全側で 4000 文字に切る。
        # cargo-clippy のように冒頭がコンパイルログで埋まるツールでは肝心の lint 行が
        # tail 側に残るため、ここでは末尾側を残す方針にする。
        if len(msg) > 4000:
            msg = "..." + msg[-4000:]
        # GitHub Actions の Annotations パネルに表示させるには file= が必要なため、
        # 仮パスとして workflow 自身を指す (パネルから該当ステップに飛べる)。
        safe_print(
            f"::error file=.github/workflows/ci.yaml,line=1,title=pyfltr {cmd} ({status})::{msg}"
        )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # pragma: no cover - 最終セーフティネット
        sys.stderr.write(f"pyfltr-annotate.py crashed: {exc}\n")
        # 失敗時診断ステップを落とさず、アノテーションには残す。
        safe_print(
            f"::warning file=.github/workflows/ci.yaml,line=1,title=pyfltr-annotate.py crashed::{exc}"
        )
        sys.exit(0)
