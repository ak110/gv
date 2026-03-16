# Phase 8 実装計画: ファイル操作・マーク・ブックマーク

## 概要

Phase 8では以下の機能群を実装する:
1. マーク機能（設定/解除/反転/一括操作）
2. ファイル操作（削除/移動/コピー/リストから削除）
3. ブックマーク（保存/復元/エディタで開く）
4. 画像書き出し（JPG/BMP/PNG）
5. クリップボード操作（画像コピー/ファイル名コピー/貼り付け）
6. 画像情報表示（メタデータ: PNGコメント、EXIF含む）
7. ナビゲーション補足（フォルダ移動、マーク移動、ページ指定）
8. ファイルを開く/フォルダを開く ダイアログ
9. ユーティリティ（各種フォルダを開く、再読み込み、新規ウィンドウ等）
10. メニューバー トグル（ToggleMenuBar）

### スコープ外（Should / 今後対応）
- 設定ダイアログ群（DialogDisplay〜DialogKeys）→ スタブ維持
- ファイルリストウィンドウ（ToggleFileList）→ スタブ維持（UIが大きいため別フェーズ）
- 履歴機能（OpenHistory）→ スタブ維持
- 一時的ソートナビゲーション（SortNavigateBack/Forward）→ スタブ維持

---

## 設計上の重要決定

### 1. 責務分離: Document (モデル層) vs AppWindow (UI層)

**原則**: DocumentはWin32 APIに一切依存しない。

- **UI層 (app.rs)**: ダイアログ表示、Shell API呼び出し、クリップボード操作を担当
- **モデル層 (document.rs)**: リスト更新、キャッシュ管理、イベント送信を担当
- **ファイル操作の流れ**:
  1. app.rs: ダイアログ表示・Shell API実行（file_ops.rs経由）
  2. app.rs: 成功したら document のリスト更新メソッドを呼ぶ
  3. document.rs: FileList更新 + キャッシュ無効化 + イベント送信

→ Documentにdelete/move/copy等のHWND引数メソッドは作らない。
   代わりにリスト操作用の低レベルメソッドを公開する。

### 2. アーカイブ内ファイルの論理ソース

現状、アーカイブ画像はtempディレクトリに展開されFileListに積まれる。
ブックマーク保存、CopyFileName、NewWindow、OpenContainingFolder等で
tempパスではなく元のアーカイブパスが必要。

**方針**: FileInfoに`source`フィールド（enum）を追加し、構造化された論理ソースを保持する。
文字列区切り（`#`等）は使わない。Windowsファイル名に任意文字が含まれるため曖昧になる。

```rust
/// ファイルの論理的なソース情報
#[derive(Debug, Clone)]
pub enum FileSource {
    /// 通常のファイルシステム上のファイル
    File(PathBuf),
    /// アーカイブ内のエントリ
    ArchiveEntry {
        archive: PathBuf,   // アーカイブファイルのパス
        entry: String,       // アーカイブ内のエントリパス
    },
}

pub struct FileInfo {
    pub path: PathBuf,          // 実ファイルパス（デコード/描画用。tempパスの場合あり）
    pub source: FileSource,     // 論理ソース（表示・保存・ブックマーク用）
    // ... 既存フィールド
}
```

- 通常ファイル: source = FileSource::File(path.clone())
- アーカイブ内: source = FileSource::ArchiveEntry { archive, entry }
  - Document.open_archive()でtempに展開時にsourceを設定

**各機能での使い分け:**
- CopyFileName: source.display_path() で表示用パスを生成
  - File → そのまま表示
  - ArchiveEntry → "archive_path > entry_path" 形式
- NewWindow: アーカイブ内の場合はアーカイブパスを引数に渡す（エントリ指定なし）
- OpenContainingFolder: アーカイブ内の場合はアーカイブファイルを /select で開く
- ブックマーク: 構造化テキスト形式で保存（後述）

**ブックマーク形式**（構造化）:
```
# gv3 bookmark v1
# index: 42
file	C:\path\to\image1.jpg
archive	C:\archive.zip	folder/image2.png
```

- タブ区切りでタイプとパスを分離
- `file\tpath` — 通常ファイル
- `archive\tarchive_path\tentry_path` — アーカイブエントリ
- タブ区切りなのでパスに含まれる`#`等の特殊文字に影響されない

### 3. 画像情報表示でメタデータを含める

既存のImageDecoder.metadata()経由でPNGコメント・EXIF等を取得し、
MessageBoxまたは簡易ダイアログで表示する。
DecoderChainにmetadata()メソッドを追加（decode()と同じフォールバック戦略）。

---

## サブフェーズ構成

### 8-A: マーク機能 + マークナビゲーション

**変更ファイル:** `file_list.rs`, `document.rs`, `app.rs`

#### file_list.rs 追加メソッド:
```rust
// 現在のファイルをマーク（マーク後に次へ移動）
pub fn mark_at(&mut self, index: usize)
// マーク解除
pub fn unmark_at(&mut self, index: usize)
// 全マーク反転
pub fn invert_all_marks(&mut self)
// 最初から現在位置までのマーク反転
pub fn invert_marks_to_here(&mut self)
// マーク済みファイルのインデックス一覧
pub fn marked_indices(&self) -> Vec<usize>
// マーク済みファイルの数
pub fn marked_count(&self) -> usize
// マーク済みファイルをリストから削除（インデックスは再計算）
pub fn remove_marked(&mut self) -> Vec<FileInfo>
// 指定インデックスをリストから削除
pub fn remove_at(&mut self, index: usize) -> Option<FileInfo>
// 前/次のマーク画像へ移動
pub fn navigate_prev_mark(&mut self) -> bool
pub fn navigate_next_mark(&mut self) -> bool
```

#### document.rs 追加メソッド:
```rust
pub fn mark_current(&mut self)       // 現在ファイルをマーク→次へ
pub fn unmark_current(&mut self)     // 現在ファイルのマーク解除
pub fn invert_all_marks(&mut self)
pub fn invert_marks_to_here(&mut self)
pub fn navigate_prev_mark(&mut self)
pub fn navigate_next_mark(&mut self)
pub fn remove_current_from_list(&mut self)  // リストから削除
pub fn remove_marked_from_list(&mut self)   // マーク済みをリストから削除
// app.rsのファイル操作用
pub fn file_list_mut(&mut self) -> &mut FileList
pub fn after_list_change(&mut self)  // キャッシュ無効化+イベント送信+再読込
```

#### app.rs:
- MarkSet, MarkUnset, MarkInvertAll, MarkInvertToHere のハンドラ実装
- MarkedRemoveFromList のハンドラ実装
- RemoveFromList のハンドラ実装
- NavigatePrevMark, NavigateNextMark のハンドラ実装

**テスト:** file_list.rsにマーク関連テストを追加

---

### 8-B: ファイル操作（削除/移動/コピー）+ ダイアログ

**新規ファイル:** `src/file_ops.rs` — Win32 Shell APIによるファイル操作 + ダイアログ
**変更ファイル:** `main.rs`, `app.rs`, `Cargo.toml`

#### file_ops.rs:
```rust
// ごみ箱経由でファイル削除（SHFileOperationW, FOF_ALLOWUNDO）
pub fn delete_to_recycle_bin(hwnd: HWND, paths: &[&Path]) -> Result<()>
// ファイル移動（SHFileOperationW）
pub fn move_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<()>
// ファイルコピー（SHFileOperationW）
pub fn copy_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<()>
// ファイル選択ダイアログ（IFileOpenDialog）
pub fn open_file_dialog(hwnd: HWND) -> Result<Option<PathBuf>>
// フォルダ選択ダイアログ（IFileOpenDialog + FOS_PICKFOLDERS）
pub fn open_folder_dialog(hwnd: HWND) -> Result<Option<PathBuf>>
// 移動/コピー先フォルダ選択ダイアログ
pub fn select_folder_dialog(hwnd: HWND, title: &str) -> Result<Option<PathBuf>>
// 保存先ダイアログ（拡張子フィルタ付き）
pub fn save_file_dialog(hwnd: HWND, default_name: &str, filter_name: &str, ext: &str) -> Result<Option<PathBuf>>
```

#### Cargo.toml追加features:
```toml
"Win32_UI_Shell_Common"
```

#### app.rs のファイル操作フロー:
```
DeleteFile:
  1. file_ops::delete_to_recycle_bin(hwnd, &[path])
  2. document.remove_current_from_list()
  3. process_document_events()

MarkedDelete:
  1. マーク済みパス一覧を収集
  2. file_ops::delete_to_recycle_bin(hwnd, &paths)
  3. document.remove_marked_from_list()

MoveFile:
  1. file_ops::select_folder_dialog() で移動先取得
  2. file_ops::move_files(hwnd, &[path], dest)
  3. document.remove_current_from_list()

CopyFile:
  1. file_ops::save_file_dialog() で保存先取得
  2. file_ops::copy_files(hwnd, &[path], dest)
  (リストは変更しない)

OpenFile:
  1. file_ops::open_file_dialog()
  2. document.open(path)

OpenFolder:
  1. file_ops::open_folder_dialog()
  2. document.open_folder(path)
```

注意: アーカイブ内ファイルの削除/移動は無効（tempファイルのため）。
app.rs側でアーカイブ内かチェックしてスキップ。

---

### 8-C: クリップボード操作

**新規ファイル:** `src/clipboard.rs`
**変更ファイル:** `main.rs`, `app.rs`, `Cargo.toml`

#### clipboard.rs:
```rust
// 画像をクリップボードにコピー（CF_DIB形式）
pub fn copy_image_to_clipboard(hwnd: HWND, image: &DecodedImage) -> Result<()>
// テキストをクリップボードにコピー（CF_UNICODETEXT形式）
pub fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> Result<()>
// クリップボードから画像を取得（CF_DIB形式）
pub fn paste_image_from_clipboard(hwnd: HWND) -> Result<Option<DecodedImage>>
```

#### Cargo.toml追加features:
```toml
"Win32_System_DataExchange"  # Clipboard API
"Win32_System_Ole"           # CF_DIB
```

#### app.rs:
- CopyImage: 現在の画像をクリップボードへ
- CopyFileName: 現在のsource_path（論理パス）をクリップボードへ
- MarkedCopyNames: マーク済みファイルのsource_path一覧をクリップボードへ
- PasteImage: クリップボードから画像を取得して表示（一時ファイル経由でリストに追加）

---

### 8-D: 画像書き出し

**変更ファイル:** `app.rs`

#### app.rs:
- ExportJpg: save_file_dialog → image crateでJPEG書き出し
- ExportBmp: save_file_dialog → image crateでBMP書き出し
- ExportPng: save_file_dialog → image crateでPNG書き出し

画像書き出しロジックはapp.rsのヘルパーメソッドに実装。
DecodedImage (RGBA Vec<u8>) → image::RgbaImage → encode して書き出し。

---

### 8-E: ブックマーク

**新規ファイル:** `src/bookmark.rs`
**変更ファイル:** `main.rs`, `document.rs`, `app.rs`

#### bookmark.rs:
```
# gv3 bookmark v1
# index: 42
file	C:\path\to\image1.jpg
archive	C:\archive.zip	folder/image2.png
```

- タブ区切りでタイプとパスを構造化
- `file\tpath` — 通常ファイル
- `archive\tarchive_path\tentry_path` — アーカイブエントリ

```rust
pub struct BookmarkData {
    pub entries: Vec<FileSource>,  // FileSource enum を再利用
    pub index: usize,
}

// ブックマーク保存
pub fn save_bookmark(hwnd: HWND, file_list: &FileList, current_index: Option<usize>) -> Result<()>
// ブックマーク読み込み
pub fn load_bookmark(hwnd: HWND) -> Result<Option<BookmarkData>>
// ブックマークフォルダのパス取得
pub fn bookmark_dir() -> PathBuf
```

#### document.rs 追加:
```rust
pub fn load_bookmark_data(&mut self, data: BookmarkData) -> Result<()>
```

#### app.rs:
- BookmarkSave: 保存ダイアログ → bookmark::save_bookmark
- BookmarkLoad: 開くダイアログ → bookmark::load_bookmark → document.load_bookmark_data
- BookmarkOpenEditor: ブックマークフォルダをエクスプローラで開く

---

### 8-F: 画像情報表示 + ユーティリティ

**変更ファイル:** `app.rs`, `document.rs`, `image/mod.rs`

#### 画像情報ダイアログ:
1. DecoderChainにmetadata()メソッドを追加（decode()と同じフォールバック戦略）
2. document.rsにcurrent_metadata()メソッドを追加
   - 現在のファイルを読んでDecoderChain.metadata()を呼ぶ
3. app.rsでShowImageInfo:
   - ファイルパス（source_path）
   - 画像サイズ（W×H）
   - ファイルサイズ
   - フォーマット名
   - PNGコメント / EXIF情報（ImageMetadata.commentsから）
   - MessageBoxで整形して表示

#### ユーティリティ:
- Reload: document.reload()（現在のファイルを再読み込み）
- NewWindow: 現在のexeを新規プロセスで起動（source_pathを引数に渡す）
- CloseAll: document.close_all() — ファイルリストクリア+画像クリア
- OpenContainingFolder: ShellExecuteWでエクスプローラを開く（/select,パス）
  - アーカイブ内の場合はアーカイブファイルをselect
- OpenExeFolder, OpenBookmarkFolder, OpenSpiFolder, OpenTempFolder: ShellExecuteWでフォルダを開く
- ToggleMenuBar: メニューバー表示/非表示（SetMenu(hwnd, NULL) / SetMenu(hwnd, hmenu)）

#### document.rs 追加:
```rust
pub fn reload(&mut self) -> Result<()>
pub fn close_all(&mut self)
pub fn current_metadata(&self) -> Result<ImageMetadata>
```

---

### 8-G: フォルダナビゲーション

**変更ファイル:** `file_list.rs`, `document.rs`, `app.rs`

#### file_list.rs 追加:
```rust
// 前/次のフォルダの最初のファイルへ移動
// フォルダ=親ディレクトリが異なるファイル群
pub fn navigate_prev_folder(&mut self) -> bool
pub fn navigate_next_folder(&mut self) -> bool
```

#### app.rs:
- NavigatePrevFolder, NavigateNextFolder のハンドラ実装

---

### 8-H: ページ指定ナビゲーション

**変更ファイル:** `app.rs`

#### app.rs:
- NavigateToPage: 入力ダイアログ（簡易テキスト入力）でページ番号を指定して移動
- Win32のシンプルなダイアログでページ番号入力を受け付ける

---

## 準備: FileSource + アーカイブAPI変更（Phase 8開始前）

全サブフェーズの前提となる2つの変更を先に行う。

### A. FileSource enum + FileInfo.source 導入

1. `FileSource` enumを`file_info.rs`に定義
   ```rust
   #[derive(Debug, Clone)]
   pub enum FileSource {
       File(PathBuf),
       ArchiveEntry { archive: PathBuf, entry: String },
   }
   ```
   - `display_path() -> String`: 表示用パスを生成
     - File → パスをそのまま表示
     - ArchiveEntry → "archive_path > entry_path" 形式
   - `archive_path() -> Option<&Path>`: アーカイブパスを返す
   - `is_archive_entry() -> bool`
2. FileInfo に `source: FileSource` を追加
3. FileInfo::from_path() で `source = FileSource::File(path.clone())`
4. Document::current_source() → &FileSourceを返す新メソッド
5. update_title() でsource.display_path()を使用（tempパスの表示を避ける）
6. main.rsのCLI引数処理は変更不要（通常のファイルパスのみ受け付ける）
   - NewWindowでアーカイブ内の場合はアーカイブパスを渡す

### B. アーカイブ展開APIの変更

現行の`extract_images()`は展開ファイル数(usize)しか返さず、元エントリ名と
tempパスのマッピング情報がない。フラット化+リネーム処理があるため、
tempファイル名から元エントリ名を復元できない。

1. `ArchiveHandler::extract_images()` の戻り値を変更:
   ```rust
   /// 展開結果: (展開先tempパス, アーカイブ内エントリパス) のペア一覧
   fn extract_images(&self, archive_path: &Path, target_dir: &Path) -> Result<Vec<(PathBuf, String)>>;
   ```

2. 各ハンドラ (zip.rs, rar.rs, sevenz.rs, susie.rs) を修正:
   展開後に `(temp_path, original_entry_path)` のペアを返す

3. `ArchiveManager::extract_images()` も同様に戻り値変更

4. `Document::open_archive()` を修正:
   - `populate_from_folder()` は使わず、マッピング結果から直接FileInfoを生成
   ```rust
   fn open_archive(&mut self, archive_path: &Path) -> Result<()> {
       let entries = self.archive_manager.extract_images(archive_path, &temp_dir)?;
       if entries.is_empty() { bail!("画像がない"); }
       self.file_list.clear();
       for (temp_path, entry_name) in &entries {
           let mut info = FileInfo::from_path(temp_path)?;
           info.source = FileSource::ArchiveEntry {
               archive: archive_path.to_path_buf(),
               entry: entry_name.clone(),
           };
           self.file_list.push(info);
       }
       self.file_list.sort_current();
       // ...
   }
   ```

5. FileListに`push()`, `clear()`, `sort_current()` メソッドを追加

---

## 実装順序

0. **準備: FileSource + アーカイブAPI変更** — 全サブフェーズの前提
1. **8-A: マーク機能** — 他機能の前提（マーク一括操作で使用）
2. **8-G: フォルダナビゲーション** — FileList拡張の続き
3. **8-B: ファイル操作** — file_ops.rs新規作成、Win32ダイアログ
4. **8-C: クリップボード** — clipboard.rs新規作成
5. **8-D: 画像書き出し** — file_ops.rsの保存ダイアログ活用
6. **8-E: ブックマーク** — bookmark.rs新規作成
7. **8-F: 画像情報 + ユーティリティ** — 残りのアクション実装
8. **8-H: ページ指定** — 入力ダイアログ

各サブフェーズ完了ごとにビルド確認 + テスト実行。
