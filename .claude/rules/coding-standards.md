---
paths:
  - "src/**/*.rs"
---

# コーディング規約

## ロックのpoison

`Mutex::lock()` / `RwLock::read()` / `RwLock::write()`のpoisonは
「他スレッドがロック保持中にパニックした」ことを示し、
これは不変条件違反とみなしてプロセスを止めるのが安全である。

- `expect("<lock 名> lock poisoned")`形式でpanicさせる（Rust標準ライブラリも同様の慣例）
- メッセージは`"<lock 名> lock poisoned"`形式で統一する。
  ログでの追跡が容易になる
- 上記方針はsusie系を含むすべてのモジュールに適用する。
  `map_err`で`anyhow::Error`化する旧パターンは禁止し、新規・既存ともに`expect`形式へ揃える

## 配布TOMLのSSOT

配布TOMLが正規ソースとなる種別（キーバインド・既定ソート種別など）は、
TOML側を唯一のSSOTとし、ソースコード内に同等のハードコードデフォルトを置かない。

- ビルド時は`include_str!`で取り込み、起動時にパースして反映する
- Rust側の`Default`実装と配布TOMLの既定値が一致することを単体テストで保証する
- 配布TOMLが正しくパースできることも単体テストで保証する

例として、デフォルトキーバインドは`ぐらびゅ.keys.default.toml`を唯一のSSOTとして管理する。
パース可否と既定値の一致は`KeyConfig::parse_toml`の単体テスト
（`default_toml_parses_and_resolves`）で検証する。

## ファイル名の自然順比較

ファイル名の自然順比較は`shlwapi.dll`の`StrCmpLogicalW`を使う。
Windowsエクスプローラーの並びと一致させるため、先頭ゼロ付き数値の扱いがエクスプローラーと
乖離するクロスプラットフォーム実装（`natord`等）は採用しない。

## ZIPアーカイブのファイル名エンコーディング

zipクレートv8の`ZipFile::name()`はファイル名をまずUTF-8として解釈し、失敗時にCP437へフォールバックする。
日本語Windowsで作成されたZIPはUTF-8フラグ（汎用ビットフラグの第11ビット）が立たずに
ファイル名がCP932で記録されるため、`name()`はCP932バイト列をCP437として誤デコードした文字列を返す。

- 表示用ファイル名は`name_raw()`で生バイトを取得し、UTF-8として有効ならUTF-8、
  そうでなければWindows API `MultiByteToWideChar`にコードページ932を指定して復号する
- ZIPエントリへの再アクセスは文字列キー（`by_name`）を使わず、`by_index`に統一する。
  zipクレートv8の`by_name`は内部で`name.as_bytes()`をキーにIndexMap（キーは原バイト列）を検索する仕様であり、
  CP437誤デコード後の文字列ではキーが一致しない
- アーカイブ内エントリの構造体には再アクセス用のインデックスを必ず持たせ、
  表示用名前と内部キーの責務を型レベルで分離する

## エラーハンドリング

- ユーザーが明示的に実行した操作（ファイル移動・コピー・保存・クリップボード操作など）が失敗した場合は、
  必ず`show_error_title()`でタイトルバーにエラーを表示する。
  `eprintln!`のみでの出力は禁止（ユーザーから視認できない）
- Susieプラグインのロード・設定ファイルのパースなど、バックグラウンド処理や初期化時のエラーは
  `eprintln!`でstderrに出力する（デバッグ用）。
  フォールバック動作がある場合はそのまま続行してよい
- 可能な限り`Result`で呼び出し元に返し、`app.rs`のアクションハンドラでエラー表示を行う。
  中間層でエラーを握り潰さない

## モーダルダイアログ表示前のカーソル可視化

フルスクリーンモードのカーソル自動非表示の状態でモーダルダイアログを開いた際は、
ダイアログ表示中もカーソルが不可視のままになる事象が発生する。
`AppWindow`メソッドからモーダルダイアログを開く場合は
`prepare_modal_dialog`と`finish_modal_dialog`のペアで囲む。

- 事前条件判定の後・ダイアログ呼び出しの直前で`self.prepare_modal_dialog()`を呼ぶ
- ダイアログ呼び出しの直後で`self.finish_modal_dialog()`を呼ぶ
- 対象は`file_ops::open_file_dialog`などのCommon Item Dialog系関数、
  `util::show_message_box`・`ui::info_dialog::show_info_dialog`などの自作モーダル、
  `file_ops::delete_to_recycle_bin`などの`IFileOperation`系関数を含む
- `bookmark::save_bookmark`などが内部でこれらの関数を呼ぶ場合は
  呼び出し元の`AppWindow`メソッド側でペアを配置する
- 早期returnを含む全終了経路で`finish_modal_dialog`を通す。ただし`DestroyWindow(self.hwnd)`後は呼ばずに関数を抜ける
- `Document`など`&self`借用を保持したままペアを呼ぶと借用検査に違反するため、
  ダイアログ呼び出し前に必要な値を所有値として取り出す
- 単一メソッド内で複数のダイアログを順次呼ぶ場合は最初の直前で`prepare`、最後の直後で`finish`と1回ずつでよい
