"""LinuxからWindows向けにクロスビルドする環境を構築する（`mise run setup-linux`から起動）。

Debian/Ubuntuを前提に、cargo-xwinが使うLLVMツール、Rustのターゲット、cargo-xwinを導入する。
続けて`cargo xwin env`が示す環境変数をGit管理外の`.mise.local.toml`へ保存し、
以後の`mise run test`・`mise run build`とpre-commitのpyfltr hookがWindowsターゲットでビルドするようにする。
cargo-xwinはMicrosoftのCRT・Windows SDKを取得し、その利用はMicrosoftのライセンスへの同意を前提とする。

再実行しても同じ結果になるよう、`.mise.local.toml`は毎回全体を生成し直す。
"""

import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib

_TARGET = "x86_64-pc-windows-msvc"
_TARGET_ENV = _TARGET.upper().replace("-", "_")
_ROOT = pathlib.Path(__file__).resolve().parent.parent
_OUTPUT = _ROOT / ".mise.local.toml"
_RUNNER = _ROOT / "scripts" / "cargo_runner_linux.sh"
_EXPORT_PATTERN = re.compile(r'^export ([A-Za-z_][A-Za-z0-9_]*)="(.*)";$')

# `cargo xwin env`の出力のうち保存しない変数。
# - RUSTFLAGS: 空値の出力であり、ユーザーが設定した値を上書きしてしまう
# - テスト実行器: wineの代わりに、テストを実行しない`scripts/cargo_runner_linux.sh`を設定する
# - WINEDEBUG: wineを使わないため不要
# - CMAKE_GENERATOR・CMAKE_SYSTEM_NAME: ターゲットを限定しないため、ホスト向けのビルドにも作用する（gvの依存はcmakeを使わない）
_EXCLUDED = {
    "RUSTFLAGS",
    f"CARGO_TARGET_{_TARGET_ENV}_RUNNER",
    "WINEDEBUG",
    "CMAKE_GENERATOR",
    "CMAKE_SYSTEM_NAME",
}


def main() -> None:
    """環境を構築し、`.mise.local.toml`を保存する。"""
    env = _clean_env()
    _run(["sudo", "apt-get", "install", "-y", "clang", "lld", "llvm"], env)
    _run(["rustup", "target", "add", _TARGET], env)
    _run(["cargo", "install", "--locked", "cargo-xwin"], env)
    env_output = _run(["cargo", "xwin", "env", "--target", _TARGET], env, capture=True)
    variables, paths = _parse_xwin_env(env_output, env)
    variables["CARGO_BUILD_TARGET"] = _TARGET
    rustflags_key = f"CARGO_TARGET_{_TARGET_ENV}_RUSTFLAGS"
    variables[rustflags_key] = (
        f"{variables.get(rustflags_key, '')} --cfg gv_no_rar".strip()
    )
    variables[f"CARGO_TARGET_{_TARGET_ENV}_RUNNER"] = str(_RUNNER)
    _OUTPUT.write_text(_render(variables, paths), encoding="utf-8")
    print(f"{_OUTPUT}へ保存した")


def _clean_env() -> dict[str, str]:
    """前回生成した`.mise.local.toml`の値を除いた環境を返す。

    再実行ではmiseが前回の`.mise.local.toml`を環境へ読み込んでいる。
    そのままでは`CARGO_BUILD_TARGET`によりホスト向けの`cargo install`までWindowsターゲットになり、
    `cargo xwin env`も既存の`LIB`・`PATH`へ同じ値を重ねて出力するため、前回の値を外して初回と同じ環境にする。
    """
    env = dict(os.environ)
    if not _OUTPUT.exists():
        return env
    previous = tomllib.loads(_OUTPUT.read_text(encoding="utf-8")).get("env", {})
    for name in previous:
        if name != "_":
            env.pop(name, None)
    previous_paths = previous.get("_", {}).get("path", [])
    env["PATH"] = os.pathsep.join(
        p for p in env.get("PATH", "").split(os.pathsep) if p not in previous_paths
    )
    return env


def _run(command: list[str], env: dict[str, str], capture: bool = False) -> str:
    """コマンドを実行し、失敗したら同じ終了コードで終了する。"""
    print("+ " + " ".join(command), flush=True)
    result = subprocess.run(
        command,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    if result.returncode != 0:
        print(
            f"コマンドが終了コード{result.returncode}で失敗した: {' '.join(command)}\n"
            "直前の出力で原因を確認して解消し、`mise run setup-linux`を再実行する。",
            file=sys.stderr,
        )
        sys.exit(result.returncode)
    return result.stdout if capture else ""


def _parse_xwin_env(
    output: str, env: dict[str, str]
) -> tuple[dict[str, str], list[str]]:
    """`cargo xwin env`のexport行を読み、環境変数と`PATH`へ追加された要素に分ける。

    `PATH`は実行時の`PATH`全体を含むため、追加された要素だけを取り出す。
    """
    current_path = env.get("PATH", "").split(os.pathsep)
    variables: dict[str, str] = {}
    paths: list[str] = []
    for line in output.splitlines():
        match = _EXPORT_PATTERN.match(line.strip())
        if match is None:
            continue
        name, value = match.groups()
        if name == "PATH":
            paths = [p for p in value.split(os.pathsep) if p and p not in current_path]
        elif name not in _EXCLUDED:
            variables[name] = value
    if not variables:
        print(
            'cargo xwin envの出力から`export 名前="値";`形式の環境変数を読み取れない。\n'
            f"`cargo xwin env --target {_TARGET}`を単独で実行して出力形式を確認する。"
            "cargo-xwinの版で形式が変わった場合は、`scripts/setup_linux.py`の読み取りを出力形式へ合わせてから"
            "`mise run setup-linux`を再実行する。",
            file=sys.stderr,
        )
        sys.exit(1)
    return variables, paths


def _render(variables: dict[str, str], paths: list[str]) -> str:
    """`.mise.local.toml`の本文を生成する。値はJSON文字列としてTOMLの基本文字列へ埋め込む。"""
    lines = [
        "# scripts/setup_linux.py が生成するLinuxクロスビルド用の設定（Git管理外）。",
        "# 再生成は`mise run setup-linux`で行う。",
        "[env]",
    ]
    if paths:
        lines.append(f"_.path = [{', '.join(json.dumps(p) for p in paths)}]")
    lines.extend(
        f"{name} = {json.dumps(value)}" for name, value in sorted(variables.items())
    )
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
