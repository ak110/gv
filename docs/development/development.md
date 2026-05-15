# 開発ガイド

## 開発環境の構築手順

### 必要環境

- mise
- Visual Studio Build Tools（C++ ビルドツール）

### 初回セットアップ

```cmd
mise install && mise run setup
```

## 開発コマンド

| コマンド          | 説明                                                        |
| ----------------- | ----------------------------------------------------------- |
| `mise run setup`  | 開発環境のセットアップ                                      |
| `mise run format` | フォーマット + 軽量lint（開発時の手動実行用。自動修正あり） |
| `mise run test`   | 全チェック実行（これを通過すればコミット可能）              |
| `mise run build`  | リリースビルド                                              |
| `mise run clean`  | ビルド成果物の削除                                          |
| `mise run update` | 依存パッケージの更新                                        |
| `mise run docs`   | ドキュメントのローカルプレビュー                            |

Linux環境ではlint系（textlint / markdownlint / prettier）のみ確認可能。
cargo-clippy / cargo-test / cargo-denyはWindowsターゲットのためLinuxでは失敗する。

## サプライチェーン攻撃対策

ロック尊重・公開待機・ピン留め運用の3点を基本方針とする。

`cargo-deny`（`deny.toml`設定）でライセンスチェックと脆弱性アドバイザリチェックを実施する。
`mise run test`に組み込まれているため、コミット前に自動実行される。

GitHub Actionsのワークフローは`pinact`でハッシュピン留めして実行する
（`mise run update`でハッシュピン更新が可能）。

## ドキュメントサイト運用

ドキュメントはGitHub Pagesでホストする（URL: <https://ak110.github.io/gv/>）。

- ローカルプレビュー: `mise run docs`
- 自動デプロイ: masterブランチへのpush時に`Docs`ワークフローが自動実行される（`docs/`以下または`package.json`の変更時のみ）

## リリース手順

GitHub Actionsの`Release`ワークフローを手動実行してリリースする。

```cmd
rem リリース実行 (いずれか1つ)
gh workflow run release.yaml --field="bump=PATCH"
gh workflow run release.yaml --field="bump=MINOR"
gh workflow run release.yaml --field="bump=MAJOR"

rem ワークフロー完了を待ち、バージョンバンプコミットを取り込む
for /f "usebackq" %i in (`gh run list --workflow=release.yaml -L1 --json=databaseId -q ".[0].databaseId"`) do gh run watch %i && git pull
```

結果の確認: <https://github.com/ak110/gv/actions>
