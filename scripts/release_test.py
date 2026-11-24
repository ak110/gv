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
    with tempfile.TemporaryDirectory(prefix="gv_release_test_") as work:
        _check_prepare_resume(_repository(pathlib.Path(work)))


def _check_prepare_resume(repository: pathlib.Path) -> None:
    source = _git(repository, "rev-parse", "HEAD")
    before = tomllib.loads((repository / "Cargo.lock").read_text(encoding="utf-8"))
    first = _prepare_cli(repository, source)
    assert first == {"version": "1.2.4", "tag": "v1.2.4", "rerun": "false"}
    after = tomllib.loads((repository / "Cargo.lock").read_text(encoding="utf-8"))
    next(
        package
        for package in before["package"]
        if package["name"] == "gv" and "source" not in package
    )["version"] = "1.2.4"
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
    assert _prepare_cli(repository, released, "MAJOR") == second
    assert _git(repository, "rev-parse", "HEAD") == released


def test_prepare_rejects_unrelated_existing_tag() -> None:
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
        original_tag = _git(repository, "rev-parse", "v1.2.4")
        _git(repository, "checkout", "--detach", source)
        with (
            contextlib.chdir(repository),
            unittest.TestCase().assertRaisesRegex(
                ValueError, "この起動元のリリースではない"
            ),
        ):
            release.prepare(source, "PATCH")
        assert _git(repository, "rev-parse", "v1.2.4") == original_tag
        assert _git(repository, "rev-parse", "HEAD") == source
        assert _git(repository, "status", "--porcelain") == ""


def test_prepare_preserves_minor_and_major_updates() -> None:
    for bump, version in [("MINOR", "1.3.0"), ("MAJOR", "2.0.0")]:
        with tempfile.TemporaryDirectory(prefix="gv_release_bump_") as work:
            repository = _repository(pathlib.Path(work))
            source = _git(repository, "rev-parse", "HEAD")
            result = _prepare_cli(repository, source, bump)
            assert result["version"] == version
            assert result["tag"] == f"v{version}"


def test_publish_verifies_the_archive_actually_distributed() -> None:
    with tempfile.TemporaryDirectory(prefix="gv_release_asset_") as work:
        for route in ("create", "upload", "keep"):
            _check_publish(pathlib.Path(work), route, signed=True)


def test_publish_rejects_unattested_archive_without_writes() -> None:
    with tempfile.TemporaryDirectory(prefix="gv_release_unsigned_") as work:
        for route in ("create", "upload", "keep"):
            _check_publish(pathlib.Path(work), route, signed=False)


def _check_publish(tmp_path: pathlib.Path, route: str, *, signed: bool) -> None:
    asset = tmp_path / "gv-v1.2.4.zip"
    asset.write_bytes(b"new distribution")
    notes = tmp_path / "notes.md"
    notes.write_text("変更履歴\n", encoding="utf-8")
    state: dict[str, str | bytes] = {
        "body": "既存の変更履歴" if route != "create" else "",
        "distribution": b"existing distribution" if route == "keep" else b"",
    }
    writes: list[str] = []
    verified: list[tuple[bytes, str]] = []
    source = "tag-commit" if route == "upload" else "parent-commit"

    def fake_run(args: tuple[str, ...], **kwargs: typing.Any) -> types.SimpleNamespace:
        stdout = ""
        if args[0] == "git":
            stdout = "tag-commit parent-commit"
        elif args[1] == "api":
            stdout = "v1.2.4" if state["body"] else ""
        elif args[1:3] == ("attestation", "verify"):
            content = pathlib.Path(args[3]).read_bytes()
            assert args[4:-1] == (
                "--repo",
                "ak110/gv",
                "--signer-workflow",
                "ak110/gv/.github/workflows/release.yaml",
                "--source-digest",
            )
            if not signed or args[-1] != source:
                raise subprocess.CalledProcessError(1, args, stderr="来歴検証失敗")
            verified.append((content, args[-1]))
        elif args[2] == "view":
            distribution = state["distribution"]
            assets = (
                [{"name": asset.name, "size": len(distribution)}]
                if distribution
                else []
            )
            stdout = json.dumps({"assets": assets, "body": state["body"]})
        elif args[2] == "download":
            _fake_download(args[3:], asset.name, state["distribution"])
        elif args[2] in {"create", "upload", "edit"}:
            assert verified
            writes.append(args[2])
            if args[2] != "edit":
                assert not state["distribution"]
                assert "--clobber" not in args
                state["distribution"] = asset.read_bytes()
            if args[2] != "upload":
                assert args[-2:] == ("--notes-file", "-")
                state["body"] = kwargs["input"]
        else:
            raise AssertionError(args)
        return types.SimpleNamespace(stdout=stdout)

    with unittest.mock.patch.object(subprocess, "run", fake_run):
        if not signed:
            with unittest.TestCase().assertRaises(subprocess.CalledProcessError):
                release.publish("ak110/gv", "v1.2.4", asset, notes)
            assert not writes
            return
        release.publish("ak110/gv", "v1.2.4", asset, notes)
        release.publish("ak110/gv", "v1.2.4", asset, notes)
    asset_bytes = b"existing distribution" if route == "keep" else asset.read_bytes()
    assert verified == [(asset_bytes, source), (asset_bytes, source)]
    assert state["distribution"] == asset_bytes
    assert (
        writes
        == {"create": ["create"], "upload": ["upload", "edit"], "keep": ["edit"]}[route]
    )
    _assert_notes(state["body"], "変更履歴" if route == "create" else "既存の変更履歴")


def _fake_download(args: tuple[str, ...], asset: str, content: str | bytes) -> None:
    assert args[:-1] == ("v1.2.4", "--repo", "ak110/gv", "--pattern", asset, "--output")
    assert isinstance(content, bytes)
    pathlib.Path(args[-1]).write_bytes(content)


def _assert_notes(body: str | bytes, original: str) -> None:
    assert isinstance(body, str)
    assert body.startswith(original)
    verify_command = (
        "gh attestation verify gv-v1.2.4.zip --repo ak110/gv "
        "--signer-workflow ak110/gv/.github/workflows/release.yaml"
    )
    assert body.count(verify_command) == 1
    assert (
        'gh api repos/ak110/gv/commits/tags/v1.2.4 --jq ".sha, .parents[0].sha"' in body
    )
    assert "--source-digest <SHA>" in body
    assert "第1親のSHAで試す" in body
    assert "検証が成功するまで展開しない" in body
    assert "https://github.com/ak110/gv/attestations" in body
    assert "gh auth login" in body and "GH_TOKEN" in body


def test_publish_does_not_treat_network_failure_as_missing_release() -> None:
    calls: list[tuple[str, ...]] = []

    def fail_run(args: tuple[str, ...], **_kwargs: typing.Any) -> typing.NoReturn:
        calls.append(args)
        raise subprocess.CalledProcessError(1, args, stderr="通信失敗")

    with tempfile.TemporaryDirectory(prefix="gv_release_failure_") as work:
        asset = pathlib.Path(work) / "gv-v1.2.4.zip"
        asset.write_bytes(b"distribution")
        with (
            unittest.mock.patch.object(subprocess, "run", fail_run),
            unittest.TestCase().assertRaises(subprocess.CalledProcessError) as raised,
        ):
            release.publish(
                "ak110/gv", "v1.2.4", asset, pathlib.Path(work) / "notes.md"
            )
    assert raised.exception.returncode == 1
    assert raised.exception.stderr == "通信失敗"
    assert len(calls) == 1
    assert calls[0][1] == "api"


def test_publish_requires_asset_after_upload() -> None:
    with tempfile.TemporaryDirectory(prefix="gv_release_verify_") as work:
        asset = pathlib.Path(work) / "gv-v1.2.4.zip"
        asset.write_bytes(b"distribution")
        tag = "v1.2.4"
        repository = "ak110/gv"
        for after_upload in ([], [{"name": asset.name, "size": 0}]):
            with (
                unittest.mock.patch.object(
                    subprocess,
                    "run",
                    side_effect=[
                        types.SimpleNamespace(stdout=tag),
                        types.SimpleNamespace(stdout="tag-commit"),
                        types.SimpleNamespace(stdout='{"assets": [], "body": ""}'),
                        types.SimpleNamespace(stdout=""),
                        types.SimpleNamespace(stdout=""),
                        types.SimpleNamespace(
                            stdout=json.dumps({"assets": after_upload})
                        ),
                    ],
                ) as run,
                unittest.TestCase().assertRaisesRegex(ValueError, "ZIPを確認できない"),
            ):
                release.publish(repository, tag, asset, pathlib.Path(work) / "notes.md")
            assert [call.args[0][:3] for call in run.call_args_list] == [
                ("gh", "api", "--paginate"),
                ("git", "rev-list", "--parents"),
                ("gh", "release", "view"),
                ("gh", "attestation", "verify"),
                ("gh", "release", "upload"),
                ("gh", "release", "view"),
            ]
