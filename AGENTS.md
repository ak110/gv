# AGENTS.md: gv

Windows用画像ビューアー（Rust製）。多形式対応と高速切り替えを軸にした単一ユーザー向けGUIアプリ。
本プロジェクトは人間による開発作業がほぼ発生しないため、
開発手順と注意点はAGENTS.md、コーディング規約は`.claude/rules/`配下、
設計判断の根拠は`docs/development/architecture.md`に置く。

## 開発手順

- rust, node, pnpmなどはmise経由で実行する
- 普段使うのは`mise run format`（フォーマット + 軽量lint）と`mise run test`（全チェック実行）
- リリースは`releaser patch`で実行する（minor・majorも同様）
  - releaserは未コミット変更と既定ブランチを確認し、pushとCI完了待機を経てリリースする
  - 導入元と初回の起動方法は[開発ガイドのリリース手順](docs/development/development.md#リリース手順)を参照する
- コミット前の検証方法: `mise run test`
  - 特定ファイルに限定する場合は `uvx --exclude-newer-package pyfltr=false pyfltr run <path>` を使う
  - 修正後の再実行時は`--commands=cargo-clippy,cargo-test`等で限定して実行する（最終検証はCIに委ねる前提）

## アーキテクチャの参照先

アーキテクチャ概要・モジュール構成・設計判断の根拠は
[docs/development/architecture.md](docs/development/architecture.md)を参照する。

## 共通品質基準

コード・設計・テスト・文書の作成と修正の前に`agent-basics:writing-standards`を起動する。
レビューとレビュー指摘の採否の判断の前には`agent-basics:review-standards`を起動する。
いずれも各スキルの読込表が示すreferences（Rustは`references/rust.md`など）を読む。
本プロジェクト固有の規約（次節）と`unsafe-reviewer`の起動条件は、共通品質基準に加えて適用する。
agent-basicsの導入は[開発ガイドの初回セットアップ](docs/development/development.md#初回セットアップ)を参照する。

## コーディング規約

Rust実装の規約（ロックpoison・TOML SSOT・自然順比較・エラーハンドリング）は
[.claude/rules/coding-standards.md](.claude/rules/coding-standards.md)に集約する。
`.claude/rules/coding-standards.md`は`src/**/*.rs`編集時に自動ロードされる。
Windowsバッチファイル生成の規約（CP932・UTF-8 BOM・chcp・goto構文）は
`src/updater.rs`の`generate_update_batch`関数のdocコメントに記載する。

## サブエージェント・スキル連携

`unsafe`ブロックを新規追加・変更した直後は、必ずサブエージェント`unsafe-reviewer`へ
対象ファイルの絶対パスを渡してレビューを依頼する。
既存の`unsafe`を含むファイルを編集しても、`unsafe`部分そのものに変更がなければ対象外。

SAFETYコメントの粒度判定基準は[.claude/agents/unsafe-reviewer.md](.claude/agents/unsafe-reviewer.md)をSSOTとする。

## 注意点

- Linux（Debian/Ubuntu）では最初に`mise run setup-linux`を実行する。
  cargo-xwinによるWindowsターゲット（x86_64-pc-windows-msvc）のクロスビルド環境がGit管理外の`.mise.local.toml`へ設定される。
  以後は`mise run test`・`mise run build`とpre-commitのpyfltr hookをWindowsと同じ手順で使える
  - LinuxビルドはRAR展開を除外する（`--cfg gv_no_rar`）。RAR関連のコードはLinuxのclippyとテストのコンパイルの対象外になる
  - Linuxの`cargo test`はテストのコンパイルまでとし、テストの実行はWindows CIが担う
  - 成果物は`target/x86_64-pc-windows-msvc/`配下へ出力される
- Makefileではなく`mise.toml`のタスクを使用する。pre-commitフレームワークは`uvx prek`で呼び出す
- Windowsの通常テストは、表示しない実Win32ウィンドウとDirect2D資源を作成・破棄する。
  `src/app/test_support.rs`の`TestApp`を使うapp層のテストと、`src/test_helpers.rs`の`HiddenWindow`を使う`src/render/d2d_renderer.rs`のテストが該当する。
  対話中の端末で反復すると、ユーザーの画面でウィンドウの開閉が起きて作業を妨げる
  - 対話中のユーザー端末では、通常の検証に必要な回数だけ実行する
  - 再現率の測定などの反復実行は、一時branchをpushしてWindows CIで行う
  - Win32ウィンドウ、Direct2D、Shell UIなど対話デスクトップのOS・UI資源を生成するテストを追加した場合も同じ扱いとする
  - Linuxの`mise run test`はテストのコンパイルまでであり、この制約の対象外である
