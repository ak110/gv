"""リリースの入力固定、タグ公開後の再開、欠落資産の補完を検証する。"""

import contextlib
import json
import pathlib
import subprocess
import sys
import tempfile
import tomllib
import types
import typing
import unittest.mock

from scripts import release


def _git(repository: pathlib.Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=repository,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=30,
    ).stdout.strip()


def _repository(tmp_path: pathlib.Path) -> pathlib.Path:
    repo = tmp_path / "repo"
    (repo / "src").mkdir(parents=True)
    (repo / "dependency" / "src").mkdir(parents=True)
    (repo / "Cargo.toml").write_text(
        '[package]\nname = "gv"\nversion = "1.2.3"\nedition = "2024"\n'
        '[dependencies]\nlocal-dependency = { path = "dependency" }\n',
        encoding="utf-8",
    )
    (repo / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
    (repo / "dependency" / "Cargo.toml").write_text(
        '[package]\nname = "local-dependency"\nversion = "0.1.0"\nedition = "2024"\n',
        encoding="utf-8",
    )
    (repo / "dependency" / "src" / "lib.rs").write_text("", encoding="utf-8")
    subprocess.run(
        ["cargo", "generate-lockfile", "--offline"], cwd=repo, check=True, timeout=30
    )
    _git(repo, "init")
    _git(repo, "config", "user.name", "release-test")
    _git(repo, "config", "user.email", "release-test@example.invalid")
    _git(repo, "config", "commit.gpgsign", "false")
    _git(repo, "add", ".")
    _git(repo, "commit", "-m", "検証済みの入力")
    return repo


def _prepare_cli(
    repository: pathlib.Path, source: str, bump: str = "PATCH"
) -> dict[str, str]:
    output = repository.parent / "action-output"
    output.unlink(missing_ok=True)
    result = subprocess.run(
        [
            sys.executable,
            str(pathlib.Path(release.__file__).resolve()),
            "prepare",
            "--source-commit",
            source,
            "--bump",
            bump,
            "--output-file",
            str(output),
        ],
        cwd=repository,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    return dict(
        line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines()
    )


def test_prepare_keeps_dependencies_and_resumes_original_dispatch() -> None:
    """初回の依存を維持し、タグpush後は元の起動commitから同じソースへ復帰する。"""
    with tempfile.TemporaryDirectory(prefix="gv_release_test_") as work:
        _check_prepare_resume(_repository(pathlib.Path(work)))


def _check_prepare_resume(repository: pathlib.Path) -> None:
    source = _git(repository, "rev-parse", "HEAD")
    before = tomllib.loads((repository / "Cargo.lock").read_text(encoding="utf-8"))
    first = _prepare_cli(repository, source)
    assert first == {"version": "1.2.4", "tag": "v1.2.4", "rerun": "false"}
    after = tomllib.loads((repository / "Cargo.lock").read_text(encoding="utf-8"))
    before["package"][0]["version"] = "1.2.4"  # gv自身の版数だけが変わる契約
    assert before == after
    subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=repository,
        check=True,
        capture_output=True,
        timeout=30,
    )
    _git(repository, "add", "Cargo.toml", "Cargo.lock")
    _git(repository, "commit", "-m", "リリースの版数")
    released = _git(repository, "rev-parse", "HEAD")
    _git(repository, "tag", first["tag"])
    _git(repository, "checkout", "--detach", source)
    second = _prepare_cli(repository, source)
    assert second == {"version": "1.2.4", "tag": "v1.2.4", "rerun": "true"}
    assert _git(repository, "rev-parse", "HEAD") == released
    assert _git(repository, "rev-list", "-n", "1", first["tag"]) == released


def test_prepare_rejects_unrelated_existing_tag() -> None:
    """同じ名前のタグでも出所が異なれば既存タグとソースを変更しない。"""
    with tempfile.TemporaryDirectory(prefix="gv_release_test_") as work:
        repository = _repository(pathlib.Path(work))
        base = _git(repository, "rev-parse", "HEAD")
        (repository / "docs" / "public").mkdir(parents=True)
        (repository / "docs" / "public" / "version.json").write_text(
            "{}", encoding="utf-8"
        )
        _git(repository, "add", ".")
        _git(repository, "commit", "-m", "別の起動元")
        source = _git(repository, "rev-parse", "HEAD")
        _git(repository, "checkout", "--detach", base)
        _prepare_cli(repository, base)
        _git(repository, "add", "Cargo.toml", "Cargo.lock")
        _git(repository, "commit", "-m", "別の起動元からのリリース")
        _git(repository, "tag", "v1.2.4")
        _git(repository, "checkout", "--detach", source)
        with (
            contextlib.chdir(repository),
            unittest.TestCase().assertRaisesRegex(
                ValueError, "この起動元のリリースではない"
            ),
        ):
            release.prepare(source, "PATCH")
        assert _git(repository, "rev-parse", "HEAD") == source
        assert _git(repository, "status", "--porcelain") == ""


def test_prepare_preserves_minor_and_major_updates() -> None:
    """既存の更新種別は、依存解決を変えずに版数へ反映する。"""
    for bump, version in [("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
        with tempfile.TemporaryDirectory(prefix="gv_release_bump_") as work:
            repository = _repository(pathlib.Path(work))
            source = _git(repository, "rev-parse", "HEAD")
            result = _prepare_cli(repository, source, bump)
            assert result["version"] == version
            assert result["tag"] == f"v{version}"


def test_publish_completes_missing_asset_without_overwriting() -> None:
    """新規公開・Releaseだけ公開済み・資産も公開済みのすべてで再実行を完遂する。"""
    with tempfile.TemporaryDirectory(prefix="gv_release_asset_") as work:
        for has_release, has_asset in [(False, False), (True, False), (True, True)]:
            _check_publish(pathlib.Path(work), has_release, has_asset)


def _check_publish(tmp_path: pathlib.Path, has_release: bool, has_asset: bool) -> None:
    asset = tmp_path / "gv-v1.2.4.zip"
    asset.write_bytes(b"distribution")
    state = {"release": has_release, "asset": has_asset}
    writes: list[str] = []

    def fake_run(args: tuple[str, ...], **_kwargs: typing.Any) -> types.SimpleNamespace:
        if args[1] == "api":
            stdout = "v1.2.4" if state["release"] else ""
        elif args[2] == "view":
            assets = (
                [{"name": asset.name, "size": asset.stat().st_size}]
                if state["asset"]
                else []
            )
            stdout = json.dumps({"assets": assets})
        elif args[2] in {"create", "upload"}:
            writes.append(args[2])
            state.update(release=True, asset=True)
            stdout = ""
        else:
            raise AssertionError(args)
        return types.SimpleNamespace(stdout=stdout)

    with unittest.mock.patch.object(subprocess, "run", fake_run):
        release.publish("ak110/gv", "v1.2.4", asset, tmp_path / "notes.md")
        release.publish("ak110/gv", "v1.2.4", asset, tmp_path / "notes.md")
    assert state == {"release": True, "asset": True}
    assert len(writes) == int(not has_asset)


def test_publish_does_not_treat_network_failure_as_missing_release() -> None:
    """一覧取得不能は新規作成の認可にならず、その場で失敗する。"""
    calls: list[tuple[str, ...]] = []

    def fail_run(args: tuple[str, ...], **_kwargs: typing.Any) -> typing.NoReturn:
        calls.append(args)
        raise subprocess.CalledProcessError(1, args, stderr="通信失敗")

    with tempfile.TemporaryDirectory(prefix="gv_release_failure_") as work:
        asset = pathlib.Path(work) / "gv-v1.2.4.zip"
        asset.write_bytes(b"distribution")
        with (
            unittest.mock.patch.object(subprocess, "run", fail_run),
            unittest.TestCase().assertRaises(subprocess.CalledProcessError),
        ):
            release.publish(
                "ak110/gv", "v1.2.4", asset, pathlib.Path(work) / "notes.md"
            )
    assert len(calls) == 1
    assert calls[0][1] == "api"


def test_publish_requires_asset_after_upload() -> None:
    """アップロードコマンド成功だけでは完了とせず、Release上の資産を確認する。"""

    def fake_run(args: tuple[str, ...], **_kwargs: typing.Any) -> types.SimpleNamespace:
        stdout = "v1.2.4" if args[1] == "api" else json.dumps({"assets": []})
        return types.SimpleNamespace(stdout=stdout)

    with tempfile.TemporaryDirectory(prefix="gv_release_verify_") as work:
        asset = pathlib.Path(work) / "gv-v1.2.4.zip"
        asset.write_bytes(b"distribution")
        with (
            unittest.mock.patch.object(subprocess, "run", fake_run),
            unittest.TestCase().assertRaisesRegex(ValueError, "ZIPを確認できない"),
        ):
            release.publish(
                "ak110/gv", "v1.2.4", asset, pathlib.Path(work) / "notes.md"
            )
