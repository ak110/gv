import argparse
import pathlib
import re
import subprocess
import sys

_DOCS_PATTERN = re.compile(r"^(docs/.+|(.+/)?[^/]+\.md)$")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="CIで検査する範囲を判定し、文書だけの変更ならdocs、それ以外はfullを標準出力へ書く。"
    )
    parser.add_argument(
        "--base",
        default="",
        help="比較の基点のcommit。空、全桁0、存在しない、HEADの祖先でない場合はfullとする。",
    )
    args = parser.parse_args()
    print(scope(pathlib.Path.cwd(), args.base))
    sys.exit(0)


def scope(repository: pathlib.Path, base: str) -> str:
    if run_git(repository, "merge-base", "--is-ancestor", base, "HEAD").returncode != 0:
        print(
            f"基点{base!r}をHEADの祖先と確かめられないため全体を検査する",
            file=sys.stderr,
        )
        return "full"
    diff = run_git(repository, "diff", "--name-only", "-z", base, "HEAD")
    if diff.returncode != 0:
        print(
            f"差分を取得できないため全体を検査する: {diff.stderr.strip()}",
            file=sys.stderr,
        )
        return "full"
    changed = [name for name in diff.stdout.split("\0") if name]
    tracked = run_git(
        repository, "ls-files", "-z", "--", "*.rs", "build.rs", "Cargo.toml"
    )
    if tracked.returncode != 0:
        return "full"
    rust_sources = [
        (repository / name).read_text(encoding="utf-8")
        for name in tracked.stdout.split("\0")
        if name
    ]
    return classify(changed, rust_sources)


def classify(changed: list[str], rust_sources: list[str]) -> str:
    if not changed:
        return "full"
    for name in changed:
        if not _DOCS_PATTERN.match(name):
            return "full"
        basename = name.rsplit("/", 1)[-1]
        if any(basename in source for source in rust_sources):
            return "full"
    return "docs"


def run_git(repository: pathlib.Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repository), *args],
        capture_output=True,
        encoding="utf-8",
        check=False,
        timeout=60,
    )


if __name__ == "__main__":
    main()
