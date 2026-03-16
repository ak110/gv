# 開発ガイド

## 必要環境

- [rustup](https://rustup.rs/)（Rust ツールチェーン）
- Visual Studio Build Tools（C++ ビルドツール）

## ビルド手順

```bash
# デバッグビルド
cargo build

# リリースビルド（最適化あり）
cargo build --release
# → target/release/gv3.exe

# テスト
cargo test

# 静的解析
cargo clippy

# フォーマット
cargo fmt
```

## リリース手順

GitHub Actions の `Release` ワークフローを手動実行してリリースする。

### GitHub CLI から実行

```bash
# バグフィックスリリース (patch)
gh workflow run release.yml --field "bump=バグフィックス"

# マイナーバージョンアップ
gh workflow run release.yml --field "bump=マイナーバージョンアップ"

# メジャーバージョンアップ
gh workflow run release.yml --field "bump=メジャーバージョンアップ"
```

### Web UI から実行

1. GitHub リポジトリの **Actions** タブを開く
2. 左メニューから **Release** ワークフローを選択
3. **Run workflow** ボタンをクリック
4. バージョンの種類を選択して実行

### ワークフローの動作

1. `Cargo.toml` のバージョンを自動バンプ
2. リリースビルド (`cargo build --release`)
3. `gv3-vX.Y.Z.zip` を作成（gv3.exe, 設定テンプレート, LICENSE を同梱）
4. バージョンコミット + タグを push
5. GitHub Release を作成し、zip をアセットとして添付
