# 開発ガイド

## 開発環境の構築手順

### 必要環境

- mise
- Visual Studio Build Tools（C++ ビルドツール）

### 初回セットアップ

```cmd
mise install && mise run setup
```

### Linuxからのクロスビルド

Linux（Debian/Ubuntu）では、Windowsターゲット（x86_64-pc-windows-msvc）向けに
cargo-xwinでクロスビルドする環境を次のコマンドで構築する。

```bash
mise install && mise run setup-linux
```

`mise run setup-linux`は`sudo apt-get`でclang・lld・llvmを導入し、Rustのターゲットとcargo-xwinを導入する。
続けてcargo-xwinが取得するMicrosoftのCRT・Windows SDK（利用はMicrosoftのライセンスへの同意を前提とする）の環境を
Git管理外の`.mise.local.toml`へ保存し、`mise run setup`の手順も実行する。再実行しても同じ設定になる。

構築後は`mise run test`・`mise run build`とpre-commitのhookをWindowsと同じ手順で使える。Windowsとの違いは次のとおり。

- RAR展開を除外してビルドする（`--cfg gv_no_rar`）。LinuxでビルドしたexeはRARを開けない
- `cargo test`はテストのコンパイルまでとし、テストの実行はWindows CIが担う
- 成果物は`target/x86_64-pc-windows-msvc/`配下へ出力される（リリースexeは`target/x86_64-pc-windows-msvc/release/ぐらびゅ.exe`）

## 開発コマンド

| コマンド                                          | 説明                                                        |
| ------------------------------------------------- | ----------------------------------------------------------- |
| `mise run setup`                                  | 開発環境のセットアップ                                      |
| `mise run setup-linux`                            | Linuxからのクロスビルド環境のセットアップ（前述）           |
| `mise run format`                                 | フォーマット + 軽量lint（開発時の手動実行用。自動修正あり） |
| `mise run test`                                   | 全チェック実行（これを通過すればコミット可能）              |
| `mise run build`                                  | リリースビルド                                              |
| `mise run clean`                                  | ビルド成果物の削除                                          |
| `mise run update`                                 | 依存パッケージの更新                                        |
| `mise run docs`                                   | ドキュメントのローカルプレビュー                            |
| `mise run bench`                                  | 表示時間の測定（後述）                                      |
| `mise run bench-memory`                           | 巨大画像の表示時間とプロセスメモリーの測定                  |
| `mise run bench-compare -- <基準JSON> <比較JSON>` | 表示時間の測定結果2件の比較                                 |

## サプライチェーン攻撃対策

lockfileの尊重、公開待機、ピン留め、脆弱性監査の4点を基本方針とする。
Rustは`Cargo.lock`、ドキュメント用のpnpm依存は`pnpm-lock.yaml`を版管理する。
依存の更新は`mise run update`で行う。
公開待機は公開から一定期間たった版だけを取り込む待機であり、`pnpm-workspace.yaml`の`minimumReleaseAge`、`pyproject.toml`の`exclude-newer`、`.github/dependabot.yaml`の`cooldown`で設定する。

`cargo-deny`（`deny.toml`設定）でライセンスチェックと脆弱性アドバイザリチェックを実施する。
`mise run test`に組み込まれているため、コミット前に自動実行される。
`Dependency Audit`ワークフローは、毎週月曜日の02:23 UTCにWindowsで実行する。
手動実行にも対応し、依存を更新しない期間（lockfileが変わらない期間）に公開された脆弱性を検出する。
Rustは`cargo-deny --locked check advisories`、pnpmは開発依存を含む`pnpm audit`で監査する。
Rustの監査が失敗してもpnpmの監査を続け、各監査の検出とツールの失敗をActionsの結果に残す。

lockfileを持つデスクトップアプリとしてGitHubのsecurity alertsは有効に保つ。
Dependabotのsecurity updatesによる自動修正PRは無効にし、version updatesの設定は維持する。
監査の検出を確認してから手動で依存を更新し、Windowsのテストを通して公開する。
監査の失敗は脆弱性の検出か、通信・セットアップなどの失敗かをログで確認する。
結果は[ActionsのDependency Audit](https://github.com/ak110/gv/actions/workflows/audit.yaml)と
リポジトリのSecurityにあるDependabot alertsで確認する。

GitHub Actionsのワークフローは`pinact`でハッシュピン留めして実行する
（`mise run update`でハッシュピン更新が可能）。

## ドキュメントサイト運用

ドキュメントはGitHub Pagesでホストする（URL: <https://ak110.github.io/gv/>）。

- ローカルプレビュー: `mise run docs`
- 自動デプロイ: masterブランチへのpush時に`Docs`ワークフローが自動実行される（`docs/`以下または`package.json`の変更時のみ）
  - ビルドしたサイトのリンク（トップページのfrontmatterとナビゲーションのリンク、見出しへのリンク、外部URLを含む）の到達先をlycheeで確かめ、全て到達できた場合だけ公開する
  - 外部URLの一時的な障害（429・5xx・タイムアウト）は到達できない扱いにしない

利用者向けガイド（`docs/guide/`）の機能説明は、操作をメニュー項目名（「表示」メニューの「余白」など）、設定を`[セクション]`とキー名（`[display]`の`margin`など）で示す。
キーの割り当ては`docs/guide/navigation.md`の表とメニューの表示に、設定値の説明は配布TOMLのコメントへ導く`docs/guide/customization.md`に委ね、各ガイドへ値を書き写さない。
値や条件を書く場合は、それを決める実装か設定の名前を同じ文に置く。

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

### 巨大画像と応答回収停止中のメモリー

`mise run bench-memory`は巨大画像の連続表示と、UIが先読み応答を回収しない状態をWindows実機で測る。
先読み応答はバックグラウンドでデコードした画像をUIへ返す結果である。
回収を止めて未処理の画像が滞留する負荷条件を再現し、キャッシュと予算対象外の使用量を含む閲覧プロセス全体のメモリーを観測する。
通常のテストとCIでは実行しない。
結果は`target/gv-bench/memory-<時刻>.json`に保存する。
素材、キャッシュ予算、停止時間、各時点のプロセスメモリーと表示時間を含む。

測定の既定条件は8192×4096のJPEG8枚、キャッシュ予算192MiB、UIの応答回収停止3秒である。
素材の準備や入力パスの指定は不要である。
測定がOSの一時フォルダへ写真風のJPEGを自動生成し、別名で8枚配置して先頭画像を開く。
測定を終えるとウィンドウを閉じ、素材を削除してから結果のJSONを保存する。
削除前の失敗や中断では一時素材が残ることがある。
寸法と枚数は`src/app/benchmark.rs`の`LARGE_IMAGE_MATERIAL`、
キャッシュ予算と停止時間は`large_image_memory_benchmark`、画像内容と形式は`prepare_large_image_material`の生成処理で変更する。
同じ環境・条件で実行し、JSONの環境情報も合わせて比較する。

1枚目の描画成功後にUIの応答回収を3秒間止め、先読みが完了しても未回収の応答が残る状態を測る。
その後に応答を回収し、残り7枚へ順に移動してウィンドウを閉じる。
JSONの`samples`へ開く前の`before_open`、停止前の`before_ui_pause`、停止後の`after_ui_pause`、
回収後の`after_response_drain`、移動後の`after_navigation`、終了後の`after_window_close`の順に記録する。
各標本の`working_set_bytes`は取得時点で物理メモリーに常駐する使用量、
`private_bytes`は取得時点で他プロセスと共有しないプロセス専用の割当量で、常駐分に限定しない。
`peak_working_set_bytes`は閲覧プロセス起動からの常駐使用量の最大値である。
これらは測定プロセス自身を`Get-Process`で取得した値であり、GPU専用メモリーを含まない。
現在の常駐量、専用割当量、過去の最大値は同じ指標ではないため、比較は同じフィールド・採取時点で行う。

2026年10月1日に、この閲覧条件で2回測定した。展開画像は1枚128MiBである。
Windows 11 Pro（10.0.26200）、Ryzen 7 5800X3D、GeForce RTX 3060 Ti、Rust 1.95.0のリリースビルドを使用した。
両測定とも8枚すべて描画に成功した。

| 観測値                           | JSONの採用値                                   |    1回目 |    2回目 |
| -------------------------------- | ---------------------------------------------- | -------: | -------: |
| 閲覧プロセスのピーク常駐メモリー | `after_window_close`の`peak_working_set_bytes` | 799.5MiB | 799.5MiB |
| UI停止後のprivateメモリー        | `after_ui_pause`の`private_bytes`              | 702.1MiB | 701.7MiB |
| 応答回収後の常駐メモリー         | `after_response_drain`の`working_set_bytes`    | 301.6MiB | 301.6MiB |
| ウィンドウ終了後の常駐メモリー   | `after_window_close`の`working_set_bytes`      |  31.3MiB |  31.2MiB |
| 初回表示                         | `trials`の`scenario=initial`の`ms`             |  473.2ms |  471.9ms |
| 先読み済み画像への移動           | `trials`の`index=1`かつ`cached=true`の`ms`     |  138.4ms |  118.4ms |

メモリーのbyte値を1024²で割り、表示時間とともに小数第2位を四捨五入した。
表示時間は描画に成功した各1試行の値で、7回の移動の平均や中央値ではない。
2回目はピークの分離を検証するため素材生成プロセスだけに4GiBのバッファを追加した。閲覧側の条件は同じである。
素材側のピーク常駐メモリーは126.0MiBから4222.1MiBへ増えたが、閲覧側は表のとおり両方799.5MiBだった。
素材側のピークを閲覧側へ混入させないことを検証しており、異なる実行で閲覧側の値が完全一致する保証ではない。
この生成負荷は`GV_BENCH_MATERIAL_PADDING_MIB`環境変数へMiB単位で指定できる（既定0、4GiBなら4096）。

キャッシュ予算はアプリ全体の使用量の上限ではない。
デコード中のバッファ、表示中の画像、未回収の応答、描画資源は別にメモリーを使う。
この条件では応答回収後と終了後に使用量が減ったが、異なる画像形式やサイズで同じ上限になるとは限らない。
測定値を画像サイズの一律な許容上限とせず、全体の使用量を評価するときはこの測定を使う。

素材生成は別プロセスで行い、そのメモリーはJSONの`material_process_memory`へ分けて記録する。
閲覧プロセスのピーク値には素材生成を含めない。
メモリー取得のPowerShell起動中もUIの応答回収は止まるが、その時間は表示時間に含めていない。
OSのファイルキャッシュを排除していないため、異なる環境で比較する場合はJSONの環境・条件も確認する。

## リリース手順

`releaser`でリリースする。これは作者の[dotfiles](https://github.com/ak110/dotfiles)が提供するコマンドであり、
gvの`mise install`と`mise run setup`には含まれない。
GitとGitHub CLI（`gh`）をPATHへ追加し、`gh auth login`で公開先リポジトリへ書き込めるアカウントに認証する。
GitHub CLIは[公式の導入案内](https://cli.github.com/)から導入する。

`releaser`を導入していない環境では、gvと同じ親フォルダへdotfilesを取得する。
以下はgvのルートで実行するWindowsのコマンド例である。
`uv run --project`はdotfilesの依存とコマンドを解決し、リリース対象の作業フォルダはgvのまま保つ。

```cmd
git clone https://github.com/ak110/dotfiles.git ..\dotfiles
uv run --project "..\dotfiles" --frozen --no-dev releaser --help
```

dotfilesが既に別の場所にある場合は`..\dotfiles`をそのパスに置き換え、引用符で囲む。
空白を含むパスでも1つの引数として渡せる。

```cmd
uv run --project "C:\work folder\dotfiles" --frozen --no-dev releaser --help
```

Pythonの必要バージョンと依存はdotfilesの`pyproject.toml`と`uv.lock`に従う。
`--frozen`を付け、導入のためにロックファイルを更新しない。

既定ブランチで、未コミット変更がないことを確認してから、更新種別を1つ選ぶ。
未pushのcommitがあれば`releaser`がpushし、CI完了を待ってからリリースする。
majorはユーザーが明示的に指定した場合だけ実行する。

```cmd
uv run --project "..\dotfiles" --frozen --no-dev releaser patch
uv run --project "..\dotfiles" --frozen --no-dev releaser minor
uv run --project "..\dotfiles" --frozen --no-dev releaser major
```

dotfilesのコマンドが既にPATH上にあれば、同じ処理を`releaser patch`などで実行できる。
`releaser`はCI完了を待ち、リリースworkflowを起動・監視してローカルを同期する。
`scripts/release.py`はworkflow内部の補助処理であり、開発者は`releaser`から起動する。

結果の確認: <https://github.com/ak110/gv/actions>
