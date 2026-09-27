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

| コマンド                                          | 説明                                                        |
| ------------------------------------------------- | ----------------------------------------------------------- |
| `mise run setup`                                  | 開発環境のセットアップ                                      |
| `mise run format`                                 | フォーマット + 軽量lint（開発時の手動実行用。自動修正あり） |
| `mise run test`                                   | 全チェック実行（これを通過すればコミット可能）              |
| `mise run build`                                  | リリースビルド                                              |
| `mise run clean`                                  | ビルド成果物の削除                                          |
| `mise run update`                                 | 依存パッケージの更新                                        |
| `mise run docs`                                   | ドキュメントのローカルプレビュー                            |
| `mise run bench`                                  | 表示時間の測定（後述）                                      |
| `mise run bench-compare -- <基準JSON> <比較JSON>` | 表示時間の測定結果2件の比較                                 |

Linux環境ではlint系（textlint / markdownlint / prettier）のみ確認可能。
cargo-clippy / cargo-test / cargo-denyはWindowsターゲットのためLinuxでは失敗する。

## 表示時間の測定

`mise run bench`は変更前後の画像の表示時間を同じ条件で比べるための測定である。
Windows実機でリリースプロファイルのテスト（`app::benchmark::tests::display_benchmark`）を実行する。
GPUとウィンドウを使って測るため`#[ignore]`とし、通常の`cargo test`とCIでは実行しない。

- 入力: 測定ごとに一時フォルダへ再生成する固定内容の画像（1920×1080、16枚）。
  JPEGのフォルダ、PNGのフォルダ、同じJPEGを格納したZIP、同じJPEGをページにしたPDFの4種類
- シナリオ: 新しい非表示ウィンドウで、初回表示（ファイル指定起動と同じ読込）、続けて前方移動10回、
  直後に逆方向移動5回を行う。これを3巡する。操作の間隔は150msで、その間も先読みを進める
- 値: 表示を要求する直前から、対象画像の描画が成功した（`EndDraw`が成功した）時点までの経過時間。
  デコードだけの時間ではない。期限（10秒）内に描画が完了しなかった試行は失敗として集計から除き、件数を別に記録する。
  各試行には、要求の時点で対象が先読み済みだったかも記録する
- 結果: `target/gv-bench/display-<時刻>.json`へ保存する。対象commit（未commitの変更の有無を含む）、
  ビルドプロファイル、rustcの版、OS・CPU・GPU名、素材、測定条件、先読み設定、試行ごとの値と、
  入力種別×シナリオごとの中央値・p95を含む。OSのファイルキャッシュは排除していない
- 比較: `mise run bench-compare -- <基準JSON> <比較JSON>`で、入力種別×シナリオごとの中央値・p95の差と比率、
  失敗件数を表示する。素材・条件・環境・ビルド条件が異なる場合は、その差を先に表示する

利用者の画像フォルダへは書き込まない。

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

`releaser`でリリースする。

```cmd
rem リリース実行 (いずれか1つ)
releaser patch
releaser minor
releaser major
```

結果の確認: <https://github.com/ak110/gv/actions>
