"""`mise run bench`が保存した表示時間の測定結果2件を比較する。

入力種別×シナリオごとに、基準と比較対象の中央値・p95、その差と比率、失敗件数を表示する。
素材・測定条件・先読み設定・実行環境・ビルド条件が異なる場合は、値の比較より先にその差を表示する。
条件が異なる結果どうしの差は、変更の効果と条件の差を区別できないためである。
"""

import argparse
import json
import pathlib
import sys
import typing

# 値を比べる前に一致を確かめる項目 (結果JSONの最上位キー)
_CONDITION_KEYS = ("material", "settings", "prefetch", "window_client_size")
# 実行環境のうち、比較の前提として差を示す項目
_ENVIRONMENT_KEYS = ("profile", "rustc", "os", "cpu", "gpu")


def main() -> None:
    """2つの結果を読み込み、条件の差と値の差を表示する。"""
    parser = argparse.ArgumentParser(description="表示時間の測定結果2件を比較する。")
    parser.add_argument("base", type=pathlib.Path, help="基準とする結果JSON")
    parser.add_argument("target", type=pathlib.Path, help="比較する結果JSON")
    args = parser.parse_args()

    base = _load(args.base)
    target = _load(args.target)

    print(f"基準: {args.base} ({_describe_commit(base)})")
    print(f"比較: {args.target} ({_describe_commit(target)})")
    print()

    differences = _condition_differences(base, target)
    if differences:
        print("条件の差 (値の差には次の条件の差も含まれる):")
        for line in differences:
            print(f"  {line}")
    else:
        print("条件の差: なし")
    print()

    _print_comparison(base, target)


def _load(path: pathlib.Path) -> dict[str, typing.Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        print(f"結果を読み込めません: {path}: {e}", file=sys.stderr)
        sys.exit(2)
    if not isinstance(data, dict) or "summary" not in data:
        print(f"測定結果の形式ではありません: {path}", file=sys.stderr)
        sys.exit(2)
    return data


def _describe_commit(report: dict[str, typing.Any]) -> str:
    env = report.get("environment", {})
    commit = str(env.get("commit", "unknown"))[:10]
    dirty = " + 未commitの変更" if env.get("dirty") else ""
    return f"commit {commit}{dirty}"


def _condition_differences(
    base: dict[str, typing.Any], target: dict[str, typing.Any]
) -> list[str]:
    lines = []
    for key in _CONDITION_KEYS:
        if base.get(key) != target.get(key):
            before = json.dumps(base.get(key), ensure_ascii=False)
            after = json.dumps(target.get(key), ensure_ascii=False)
            lines.append(f"{key}: {before} -> {after}")
    base_env = base.get("environment", {})
    target_env = target.get("environment", {})
    for key in _ENVIRONMENT_KEYS:
        if base_env.get(key) != target_env.get(key):
            lines.append(
                f"environment.{key}: {base_env.get(key)} -> {target_env.get(key)}"
            )
    return lines


def _print_comparison(
    base: dict[str, typing.Any], target: dict[str, typing.Any]
) -> None:
    base_rows = {(s["input"], s["scenario"]): s for s in base["summary"]}
    target_rows = {(s["input"], s["scenario"]): s for s in target["summary"]}
    keys = list(base_rows) + [k for k in target_rows if k not in base_rows]

    print(
        f"{'入力':<12} {'シナリオ':<9} {'指標':<6} {'基準ms':>9} {'比較ms':>9} "
        f"{'差ms':>9} {'比率':>7}  失敗(基準/比較)"
    )
    for key in keys:
        b = base_rows.get(key, {})
        t = target_rows.get(key, {})
        failed = f"{b.get('failed', '-')}/{t.get('failed', '-')}"
        for metric, field in (("中央値", "median_ms"), ("p95", "p95_ms")):
            before, after = b.get(field), t.get(field)
            print(
                f"{key[0]:<12} {key[1]:<9} {metric:<6} "
                f"{_format_ms(before):>9} {_format_ms(after):>9} "
                f"{_format_diff(before, after):>9} {_format_ratio(before, after):>7}  {failed}"
            )


def _format_ms(value: float | None) -> str:
    return "-" if value is None else f"{value:.2f}"


def _format_diff(base: float | None, target: float | None) -> str:
    if base is None or target is None:
        return "-"
    return f"{target - base:+.2f}"


def _format_ratio(base: float | None, target: float | None) -> str:
    if base is None or target is None or base == 0:
        return "-"
    return f"{target / base:.2f}x"


if __name__ == "__main__":
    main()
