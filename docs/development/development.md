# 開発ガイド

## 開発環境の構築手順

### 必要環境

- mise
- Visual Studio Build Tools（C++ ビルドツール）

### 初回セットアップ

```cmd
mise install && mise run setup
```

コーディングエージェントで開発する場合は、共通品質基準のプラグイン`agent-basics`（マーケットプレイス`ak110-dotfiles`）を導入する。
Claude Codeでは、`.claude/settings.json`がマーケットプレイスの登録とプラグインの有効化をプロジェクトで共有する。
未導入の端末ではプロジェクトを開いたときに導入を案内されるため、それに従うか次のコマンドで導入する。

```cmd
claude plugin marketplace add ak110/dotfiles
claude plugin install agent-basics@ak110-dotfiles
```

Codexでは、`.codex/config.toml`がプラグインを有効にするだけで、マーケットプレイスの登録とプラグインの導入は行わない。
各端末で次のコマンドを1回実行する。

```cmd
codex plugin marketplace add ak110/dotfiles
codex plugin add agent-basics@ak110-dotfiles
```

agent-toolkitは任意であり、gvの開発に必須ではない。

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
Rustは`Cargo.lock`、ドキュメント用のpnpm依存は`pnpm-lock.yaml`、miseが導入する開発ツールは`mise.lock`を版管理する。
依存の更新は`mise run update`で行い、`mise.lock`も同じタスクの`mise lock --bump`で更新する。
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
入力は測定ごとに一時フォルダへ再生成するため、利用者の画像フォルダへは書き込まない。

値は表示を要求する直前から、対象画像の描画が成功した（`EndDraw`が成功した）時点までの経過時間であり、デコードだけの時間ではない。
結果は`target/gv-bench/`へJSONで保存し、`mise run bench-compare -- <基準JSON> <比較JSON>`で比較する。
OSのファイルキャッシュは排除していないため、異なる環境で比較する場合はJSONの環境・条件も確認する。

`mise run bench-memory`は巨大画像の連続表示と、UIが先読み応答を回収しない状態のプロセス全体のメモリーをWindows実機で測る。
キャッシュ予算はアプリ全体の使用量の上限ではない。
デコード中のバッファ、表示中の画像、未回収の応答、描画資源は別にメモリーを使うため、全体の使用量を評価するときはこの測定を使う。
素材生成側のピークを分離できることを確かめる負荷は、`GV_BENCH_MATERIAL_PADDING_MIB`環境変数へMiB単位で指定できる（既定0）。

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

タグの作成後にReleaseへZIPを載せる処理が失敗した場合は、元のRelease workflowを再実行する。
タグを指定して起動し直す場合は、先に同じタグでCIを成功させる（以下の`v1.2.3`は対象のタグに置き換える）。
自動リリースが作成したタグのcommitではpushによるCIが起動しないため、CIの実行がなければ手動で起動する。

```cmd
gh workflow run ci.yaml --ref v1.2.3
```

そのタグのCIが失敗している場合は、ログを確認し、一時的な原因なら同じタグのCIを再実行する。
タグのcommit自体の修正が必要なら同じタグのZIPは補完できない。修正後のcommitでCIを成功させ、新しい版をリリースする。

そのタグのCIが成功したことを[Actions](https://github.com/ak110/gv/actions)で確認してから、タグのcommitでReleaseを起動する。

```cmd
gh workflow run release.yaml --ref v1.2.3 -f bump=PATCH
```

既存タグからの起動では`bump`による版数の変更は行わない。

結果の確認: <https://github.com/ak110/gv/actions>
