# /// script
# requires-python = ">=3.11"
# ///
"""検証済みの依存と起動元commitを保持してリリースを準備・公開する。"""

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib


def _run(*args: str, stdin: str | None = None) -> str:
    result = subprocess.run(
        args,
        input=stdin,
        check=True,
        capture_output=True,
        encoding="utf-8",
        timeout=300,
    )
    return result.stdout.strip()


def _version(commit: str) -> str:
    return str(
        tomllib.loads(_run("git", "show", f"{commit}:Cargo.toml"))["package"]["version"]
    )


def _dependencies(lock: str) -> list[dict[str, object]]:
    return [
        package
        for package in tomllib.loads(lock)["package"]
        if package["name"] != "gv" or "source" in package
    ]


def _bumped_version(version: str, bump: str) -> str:
    major, minor, patch = (int(part) for part in version.split("."))
    match bump:
        case "PATCH":
            patch += 1
        case "MINOR":
            minor, patch = minor + 1, 0
        case "MAJOR":
            major, minor, patch = major + 1, 0, 0
        case _:
            raise ValueError(f"不明な版数更新種別: {bump}")
    return f"{major}.{minor}.{patch}"


def prepare(source_commit: str, bump: str) -> dict[str, str]:
    source = _run(
        "git",
        "rev-parse",
        "--verify",
        "--end-of-options",
        f"{source_commit}^{{commit}}",
    )
    version = _version(source)
    next_version = _bumped_version(version, bump)
    tags = _run("git", "tag", "--list").splitlines()
    if (
        f"v{version}" in tags
        and _run("git", "rev-list", "-n", "1", f"v{version}") == source
    ):
        tag, next_version = f"v{version}", version
        release_commit = source
    else:
        tag = f"v{next_version}"
        release_commit = _run("git", "rev-list", "-n", "1", tag) if tag in tags else ""
        if release_commit:
            _check_release_parent(source, release_commit, tag)
    if release_commit:
        if _version(release_commit) != next_version:
            raise ValueError(
                f"既存タグ{tag}とCargo.tomlの版数が異なる。タグを確認してください"
            )
        if _dependencies(_run("git", "show", f"{source}:Cargo.lock")) != _dependencies(
            _run("git", "show", f"{release_commit}:Cargo.lock")
        ):
            raise ValueError(
                f"既存タグ{tag}の依存が検証済みcommitと異なる。"
                f"タグのcommitのCI成功を確認してから起動し直すか、"
                f"誤ったタグなら`git push --delete origin {tag}`で削除してからリリースを再実行してください"
            )
        _run("git", "checkout", "--detach", release_commit)
        rerun = "true"
    else:
        _update_version(source, next_version)
        rerun = "false"
    return {"version": next_version, "tag": tag, "rerun": rerun}


def _check_release_parent(source: str, release_commit: str, tag: str) -> None:
    parents = _run("git", "show", "-s", "--format=%P", release_commit).split()
    changed = set(
        _run("git", "diff", "--name-only", source, release_commit).splitlines()
    )
    if parents != [source] or not changed <= {
        "Cargo.toml",
        "Cargo.lock",
        "docs/public/version.json",
    }:
        raise ValueError(
            f"既存タグ{tag}はこの起動元のリリースではない。タグを確認してください"
        )


def _update_version(source: str, next_version: str) -> None:
    if _run("git", "rev-parse", "HEAD") != source:
        raise ValueError("起動元commitをチェックアウトしてから実行してください")
    manifest = pathlib.Path("Cargo.toml")
    old_lock = pathlib.Path("Cargo.lock").read_text(encoding="utf-8")
    content, count = re.subn(
        r'(?m)^version = "\d+\.\d+\.\d+"$',
        f'version = "{next_version}"',
        manifest.read_text(encoding="utf-8"),
    )
    if count != 1:
        raise ValueError(
            "Cargo.tomlのpackage版数を一意に取得できない。manifestを確認してください"
        )
    manifest.write_text(content, encoding="utf-8")
    _run("cargo", "update", "--workspace")
    new_lock = pathlib.Path("Cargo.lock").read_text(encoding="utf-8")
    if _dependencies(old_lock) != _dependencies(new_lock):
        raise ValueError(
            "依存が検証済みロックから変わった。"
            "ローカルで`cargo update --workspace`を実行して更新したCargo.lockをcommitし、"
            "CI成功を確認してからリリースを再実行してください"
        )


def _verify_attestation(
    repository: str, asset: pathlib.Path, sources: list[str]
) -> None:
    command = (
        "gh",
        "attestation",
        "verify",
        str(asset),
        "--repo",
        repository,
        "--signer-workflow",
        f"{repository}/.github/workflows/release.yaml",
        "--source-digest",
    )
    for source in sources[:-1]:
        try:
            _run(*command, source)
        except subprocess.CalledProcessError:
            continue
        return
    _run(*command, sources[-1])


def _attestation_notes(repository: str, tag: str, asset_name: str) -> str:
    verify_command = (
        f"gh attestation verify {asset_name} --repo {repository} "
        f"--signer-workflow {repository}/.github/workflows/release.yaml"
    )
    return f"""## ZIPのビルド来歴を検証する

GitHub CLIを `gh auth login` または `GH_TOKEN` で認証し、ダウンロードしたZIPを検証する。

```text
{verify_command}
```

次に、リリースタグのcommitとその第1親（版数更新前のcommit）のSHAを取得する。出力はこの順に並ぶ。

```text
gh api repos/{repository}/commits/tags/{tag} --jq ".sha, .parents[0].sha"
```

最初の検証コマンドの末尾に `--source-digest <SHA>` を追加し、タグのSHAに置き換えて実行する。
一致しない場合は第1親のSHAで試す。どちらかで検証が成功すれば、ZIPの署名元とソースcommitの両方を確認できたことになる。
どちらでも成功しない場合は、版数・ZIP・認証を確認し、検証が成功するまで展開しない。
[証明の一覧](https://github.com/{repository}/attestations)でも確認できる。"""


def _append_notes(body: str, guidance: str) -> str:
    body = body.rstrip("\r\n")
    return f"{body}\n\n{guidance}\n" if body else f"{guidance}\n"


def publish(
    repository: str, tag: str, asset: pathlib.Path, notes: pathlib.Path
) -> None:
    if (
        not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag)
        or asset.name != f"gv-{tag}.zip"
    ):
        raise ValueError(f"タグと配布ZIPの名前が対応していない: {tag}, {asset.name}")
    if not asset.is_file() or asset.stat().st_size == 0:
        raise ValueError(f"配布ZIPがないか空である: {asset}。ZIPを再生成してください")
    releases = _run(
        "gh",
        "api",
        "--paginate",
        f"repos/{repository}/releases",
        "--jq",
        ".[].tag_name",
    ).splitlines()
    commit, *parents = _run(
        "git", "rev-list", "--parents", "-n", "1", f"refs/tags/{tag}"
    ).split()
    sources = parents[:1] + [commit]
    guidance = _attestation_notes(repository, tag, asset.name)
    updated_notes = None
    if tag not in releases:
        _verify_attestation(repository, asset, sources)
        _run(
            "gh",
            "release",
            "create",
            tag,
            str(asset),
            "--verify-tag",
            "--repo",
            repository,
            "--title",
            tag,
            "--notes-file",
            "-",
            stdin=_append_notes(notes.read_text(encoding="utf-8"), guidance),
        )
    else:
        data = json.loads(
            _run(
                "gh",
                "release",
                "view",
                tag,
                "--repo",
                repository,
                "--json",
                "assets,body",
            )
        )
        if any(item["name"] == asset.name for item in data["assets"]):
            with tempfile.TemporaryDirectory(prefix="gv_release_") as directory:
                published = pathlib.Path(directory) / asset.name
                _run(
                    "gh",
                    "release",
                    "download",
                    tag,
                    "--repo",
                    repository,
                    "--pattern",
                    asset.name,
                    "--output",
                    str(published),
                )
                _verify_attestation(repository, published, sources)
        else:
            _verify_attestation(repository, asset, sources)
            _run("gh", "release", "upload", tag, str(asset), "--repo", repository)
        body = data["body"]
        if guidance not in body.replace("\r\n", "\n"):
            updated_notes = _append_notes(body, guidance)
    data = json.loads(
        _run("gh", "release", "view", tag, "--repo", repository, "--json", "assets")
    )
    if not any(
        item["name"] == asset.name and item["size"] > 0 for item in data["assets"]
    ):
        raise ValueError(
            f"Release {tag}のZIPを確認できない。公開処理を再実行してください"
        )
    if updated_notes is not None:
        _run(
            "gh",
            "release",
            "edit",
            tag,
            "--repo",
            repository,
            "--notes-file",
            "-",
            stdin=updated_notes,
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--source-commit", required=True)
    prepare_parser.add_argument(
        "--bump", choices=["PATCH", "MINOR", "MAJOR"], required=True
    )
    prepare_parser.add_argument("--output-file", type=pathlib.Path, required=True)
    publish_parser = commands.add_parser("publish")
    publish_parser.add_argument("--repository", required=True)
    publish_parser.add_argument("--tag", required=True)
    publish_parser.add_argument("--asset", type=pathlib.Path, required=True)
    publish_parser.add_argument("--notes", type=pathlib.Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            values = prepare(args.source_commit, args.bump)
            with args.output_file.open("a", encoding="utf-8") as output:
                for key, value in values.items():
                    output.write(f"{key}={value}\n")
        else:
            publish(args.repository, args.tag, args.asset, args.notes)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(
            f"リリース処理に失敗しました: {error}。"
            "原因（直後の出力を含む）を解消してから、リリースworkflowを再実行してください",
            file=sys.stderr,
        )
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
