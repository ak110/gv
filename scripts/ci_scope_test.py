import pathlib

import pytest

from scripts import ci_scope


def _git(repository: pathlib.Path, *args: str) -> str:
    result = ci_scope.run_git(
        repository, "-c", "user.name=t", "-c", "user.email=t@example.com", *args
    )
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


def _commit(repository: pathlib.Path, files: dict[str, str]) -> str:
    for name, content in files.items():
        path = repository / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    _git(repository, "add", "--all")
    _git(repository, "commit", "-q", "-m", "change")
    return _git(repository, "rev-parse", "HEAD")


@pytest.fixture(name="repository")
def _repository(tmp_path: pathlib.Path) -> pathlib.Path:
    _git(tmp_path, "init", "-q", "-b", "main")
    _commit(
        tmp_path,
        {
            "src/main.rs": 'const GUIDE: &str = include_str!("../docs/guide/customization.md");\n',
            "docs/guide/customization.md": "# キー\n",
            "docs/guide/viewer.md": "# 表示\n",
            "README.md": "# gv\n",
        },
    )
    return tmp_path


@pytest.mark.parametrize(
    ("files", "expected"),
    [
        (
            {"docs/guide/viewer.md": "# 表示の変更\n", "README.md": "# gv 変更\n"},
            "docs",
        ),
        ({"docs/guide/customization.md": "# キーの変更\n"}, "full"),
        (
            {"docs/guide/viewer.md": "# 表示の変更\n", "src/main.rs": "fn main() {}\n"},
            "full",
        ),
        ({"Cargo.lock": "version = 4\n"}, "full"),
    ],
)
def test_scope_selects_docs_only_for_unreferenced_documents(
    repository: pathlib.Path, files: dict[str, str], expected: str
) -> None:
    base = _git(repository, "rev-parse", "HEAD")
    _commit(repository, files)
    assert ci_scope.scope(repository, base) == expected


def test_scope_falls_back_to_full_without_usable_base(repository: pathlib.Path) -> None:
    head = _git(repository, "rev-parse", "HEAD")
    _commit(repository, {"docs/guide/viewer.md": "# 表示の変更\n"})
    _git(repository, "checkout", "-q", "-b", "rewritten", head)
    _commit(repository, {"README.md": "# 書き換え\n"})
    rewritten_away = _git(repository, "rev-parse", "main")

    assert ci_scope.scope(repository, "") == "full"
    assert ci_scope.scope(repository, "0" * 40) == "full"
    assert ci_scope.scope(repository, rewritten_away) == "full"
    assert ci_scope.scope(repository, head) == "docs"


def test_scope_treats_moving_rust_source_into_docs_as_full(
    repository: pathlib.Path,
) -> None:
    base = _git(repository, "rev-parse", "HEAD")
    _git(repository, "mv", "src/main.rs", "docs/guide/example.md")
    _commit(repository, {"docs/guide/viewer.md": "# 表示の変更\n"})
    assert ci_scope.scope(repository, base) == "full"
