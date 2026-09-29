"""文書が案内するSKIP指定とprek設定のhook IDを照合する検査。

`AGENTS.md`などが案内する`SKIP=`の値は`.pre-commit-config.yaml`が定義するhook IDの転記であり、
hook構成が変わっても文書側に追随の契機が無い。不一致のまま案内どおり実行すると、
prekは該当hookが無いと警告するだけで、無効化を意図したhookをそのまま実行する。

不一致は文書の案内が成立しないことを意味するためerror区分とし、終了コード1で終わる。
prek設定の読み取りに失敗した場合は検査そのものを開始できないため、違反と区別して終了コード2で終わる。
"""

import argparse
import pathlib
import re
import subprocess
import sys

_HOOK_ID_PATTERN = re.compile(r"^\s*-\s+id:\s*(\S+)\s*$")
_SKIP_PATTERN = re.compile(r"SKIP=([A-Za-z0-9_.,-]+)")


def main() -> None:
    """照合を実行し、不一致の有無を終了コードで返す。"""
    parser = argparse.ArgumentParser(
        description="文書が案内するSKIP指定とprek設定のhook IDを照合する。"
    )
    parser.add_argument(
        "docs",
        nargs="*",
        type=pathlib.Path,
        help="走査するMarkdown。省略時はGitの追跡下にある全Markdownを対象とする。",
    )
    parser.add_argument(
        "--config",
        type=pathlib.Path,
        default=pathlib.Path(".pre-commit-config.yaml"),
        help="照合の基準とするprek設定のパス。",
    )
    args = parser.parse_args()

    hook_ids = _load_hook_ids(args.config)
    docs = args.docs if args.docs else _tracked_markdown()
    violations = _collect_violations(docs, hook_ids)
    if violations:
        for path, lineno, value in violations:
            print(
                f"{path}:{lineno}: SKIP指定の{value}は定義済みhook IDに存在しない",
                file=sys.stderr,
            )
        print(f"定義済みhook ID: {', '.join(sorted(hook_ids))}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)


def _load_hook_ids(config: pathlib.Path) -> set[str]:
    """prek設定からhook IDの集合を読む。repoの別は問わず全hookを対象とする。"""
    try:
        lines = config.read_text(encoding="utf-8").splitlines()
    except OSError as e:
        print(f"prek設定の読み取りに失敗した: {config}: {e}", file=sys.stderr)
        sys.exit(2)
    hook_ids = {m.group(1) for line in lines if (m := _HOOK_ID_PATTERN.match(line))}
    if not hook_ids:
        print(f"prek設定からhook IDを取得できない: {config}", file=sys.stderr)
        sys.exit(2)
    return hook_ids


def _tracked_markdown() -> list[pathlib.Path]:
    """Gitの追跡下にあるMarkdownの一覧を返す。"""
    try:
        proc = subprocess.run(
            ["git", "ls-files", "-z", "*.md"],
            capture_output=True,
            check=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"Gitの追跡下のMarkdownを取得できない: {e}", file=sys.stderr)
        sys.exit(2)
    return [pathlib.Path(name) for name in proc.stdout.split("\0") if name]


def _collect_violations(
    docs: list[pathlib.Path], hook_ids: set[str]
) -> list[tuple[pathlib.Path, int, str]]:
    """走査対象の各文書から、hook IDに存在しないSKIP指定を集める。"""
    violations: list[tuple[pathlib.Path, int, str]] = []
    for doc in docs:
        try:
            lines = doc.read_text(encoding="utf-8").splitlines()
        except OSError as e:
            print(f"走査対象の読み取りに失敗した: {doc}: {e}", file=sys.stderr)
            sys.exit(2)
        for lineno, line in enumerate(lines, start=1):
            for match in _SKIP_PATTERN.finditer(line):
                for value in match.group(1).split(","):
                    if value and value not in hook_ids:
                        violations.append((doc, lineno, value))
    return violations


if __name__ == "__main__":
    main()
