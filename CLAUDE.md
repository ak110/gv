# CLAUDE.md: gv

## 開発手順

- rust, node, pnpmなどはmise経由で実行する
- リリース手順: [docs/development/development.md](docs/development/development.md) 参照
- コミット前の検証方法: `uv run pyfltr run-for-agent`
  - ドキュメントなどのみの変更の場合は省略可（pre-commitで実行されるため）
  - テストコードの単体実行なども極力 `uv run pyfltr run-for-agent <path>` を使う（pytestを直接呼び出さない）
    - 詳細な情報などが必要な場合に限り `uv run pytest -vv <path>` などを使用
  - 修正後の再実行時は、対象ファイルや対象ツールを必要に応じて絞って実行する（最終検証はCIに委ねる前提）
    - 例: `pyfltr run-for-agent --commands=mypy,ruff-check path/to/file`

## 注意点

- `Mutex::lock()` / `RwLock::read()` / `RwLock::write()` のpoisonは「他スレッドがロック保持中にパニックした」
  ことを示し、これは不変条件違反とみなしてプロセスを止めるのが安全。
- そのため `expect("<lock 名> lock poisoned")` 形式でpanicさせてよい（Rust標準ライブラリも同様の慣例）。
- メッセージは `"<lock 名> lock poisoned"` 形式で統一する。これによりログでの追跡が容易になる。

### unsafe-reviewer の必須呼び出し

`unsafe`ブロックを含む`.rs`ファイルを編集・新規作成した直後は、必ず`Task`ツールで
`subagent_type=unsafe-reviewer`を呼び出し、対象ファイルの絶対パスを与えてレビューを受けること。
これは`.claude/hooks/post-edit-rust.sh`のstderrリマインダとペアになっている恒久ルールである。
`unsafe`を1行も触っていない場合でも、編集したファイルに既存の`unsafe`が含まれていれば対象となる。

## リリースビルド

- 作業完了時、コミットと並行してバックグラウンドでリリースビルドも実行する（完了を待つ必要は無い。
  ユーザーによる動作確認をスムーズにするため）
