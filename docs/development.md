# 開発ガイド

## 必要環境

- [rustup](https://rustup.rs/)（Rustツールチェーン）
- Visual Studio Build Tools（C++ ビルドツール）

## ビルド手順

```cmd
REM デバッグビルド
cargo build

REM リリースビルド（最適化あり）
cargo build --release
REM → target/release/gv3.exe

REM テスト
cargo test

REM 静的解析
cargo clippy

REM フォーマット
cargo fmt
```

## 依存パッケージの更新

```cmd
REM Cargo.lock を最新に更新（semver互換範囲内）
cargo update

REM ビルド・テスト確認
cargo build && cargo test && cargo clippy

REM メジャーバージョンアップの確認（任意）
cargo install cargo-outdated
cargo outdated
```

メジャーバージョンアップがある場合は `Cargo.toml` のバージョン指定を手動で更新する。

## リリース手順

GitHub Actionsの `Release` ワークフローを手動実行してリリースする。

### GitHub CLI から実行

```cmd
REM 1. リリース実行（いずれか1つ）
gh workflow run release.yml --field "bump=バグフィックス"
gh workflow run release.yml --field "bump=マイナーバージョンアップ"
gh workflow run release.yml --field "bump=メジャーバージョンアップ"

REM 2. ワークフロー完了を待つ（数秒待ってから実行）
for /f %i in ('gh run list --workflow=release.yml -L1 --json databaseId -q ".[0].databaseId"') do gh run watch %i

REM 3. バージョンバンプコミットを取り込む
git pull
```

> **注意**: 手順3を忘れると、ローカルの git グラフが枝分かれします。

結果の確認: <https://github.com/ak110/gv3/actions>
