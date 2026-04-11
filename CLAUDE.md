# カスタム指示 (プロジェクト固有)

## 開発手順

- rust, node, pnpmなどはmise経由で実行する

## ローカルコーディング規約

`~/.claude/rules/agent-basics/rust.md` をベースに、Win32 + COM 集約の本プロジェクトで現実的に運用するための補足。

### unsafe-reviewer の必須呼び出し

`unsafe` ブロックを含む `.rs` ファイルを編集・新規作成した直後は、必ず `Task` ツールで `subagent_type=unsafe-reviewer` を呼び出し、対象ファイルの絶対パスを与えてレビューを受けること。これは `.claude/hooks/post-edit-rust.sh` の stderr リマインダとペアになっている恒久ルールであり、`unsafe` を 1 行も触っていない場合でも、編集したファイルに既存の `unsafe` が含まれていれば対象となる。

### Mutex / RwLock の poison 扱い

- `Mutex::lock()` / `RwLock::read()` / `RwLock::write()` の poison は「他スレッドがロック保持中にパニックした」ことを示し、これは不変条件違反とみなしてプロセスを止めるのが安全。
- そのため `expect("<lock 名> lock poisoned")` 形式で panic させてよい (Rust 標準ライブラリも同様の慣例)。
- メッセージは `"<lock 名> lock poisoned"` 形式で統一する。これによりログでの追跡が容易になる。

## 関連ドキュメント

- @README.md
- @docs/development/concept.md
- @docs/development/architecture.md
- @docs/development/development.md
