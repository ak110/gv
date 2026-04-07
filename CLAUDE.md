# カスタム指示 (プロジェクト固有)

## 開発手順

- rust, node, pnpmなどはmise経由で実行する

## ローカルコーディング規約

`~/.claude/rules/agent-basics/rust.md` をベースに、Win32 + COM 集約の本プロジェクトで現実的に運用するための補足。

### `unsafe` ブロックの `// SAFETY:` 運用

- ルール本文は「すべての unsafe にコメント」だが、Win32 / COM の単純な API 呼び出し (`SendMessageW`, `SetWindowPos`, `OpenClipboard`, `GetMessageW`, COM オブジェクトの通常メソッド呼び出し等) は SAFETY コメントを省略してよい。安全性の根拠が「Microsoft ドキュメント通りの引数を渡しているだけ」となるためノイズになる。
- 以下のケースでは `// SAFETY:` を必ず付ける:
  - 生ポインタの読み書き、`ptr::read_unaligned`、バイト列の `from_raw_parts` / `transmute` 系キャスト
  - `memmap2::Mmap::map` などライフタイム外の前提に依存する操作
  - `libloading` 経由の関数呼び出し (シグネチャ一致が安全性の根拠)
  - `Send` / `Sync` を手で実装している型
  - COM オブジェクトの非自明な所有権遷移

### Mutex / RwLock の poison 扱い

- `Mutex::lock()` / `RwLock::read()` / `RwLock::write()` の poison は「他スレッドがロック保持中にパニックした」ことを示し、これは不変条件違反とみなしてプロセスを止めるのが安全。
- そのため `expect("<lock 名> lock poisoned")` 形式で panic させてよい (Rust 標準ライブラリも同様の慣例)。
- メッセージは `"<lock 名> lock poisoned"` 形式で統一する。これによりログでの追跡が容易になる。

## 関連ドキュメント

- @README.md
- @docs/concept.md
- @docs/architecture.md
- @docs/development.md
