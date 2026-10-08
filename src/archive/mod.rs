mod rar;
mod sevenz;
pub mod susie;
pub mod zip;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::extension_registry::ExtensionRegistry;

/// 組み込みハンドラの形式名と拡張子。拡張子の実体は各ハンドラが定義する。
pub fn builtin_formats() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("ZIP", zip::EXTENSIONS),
        ("RAR", rar::EXTENSIONS),
        ("7z", sevenz::EXTENSIONS),
    ]
}

/// コミック用の拡張子だけをシェルへ関連付ける。
pub fn comic_extensions() -> [&'static str; 2] {
    [zip::EXTENSIONS[1], rar::EXTENSIONS[1]]
}

/// 汎用アーカイブは過去の関連付けを解除する対象として使う。
pub fn generic_extensions() -> [&'static str; 3] {
    [
        zip::EXTENSIONS[0],
        rar::EXTENSIONS[0],
        sevenz::EXTENSIONS[0],
    ]
}

/// アーカイブ展開結果: (展開先パス, アーカイブ内エントリパス, 元の更新日時)
pub type ExtractedEntry = (PathBuf, String, std::time::SystemTime);

/// オンデマンド取得用のアーカイブ内画像エントリ情報
pub struct ArchiveImageEntry {
    /// アーカイブ内パス (例: "subfolder/image.png")
    /// ZIPの場合はCP932/UTF-8復号後の表示用文字列であり再アクセス用キーには使用できない
    pub entry_name: String,
    /// フラット化したファイル名 (ソート用)
    pub file_name: String,
    /// 非圧縮サイズ
    pub file_size: u64,
    /// アーカイブ内エントリのインデックス (再アクセス用キー)
    /// この型を生成するZIPの`ZipArchive::by_index`の引数に対応する。
    pub entry_index: u32,
    /// アーカイブ内の更新日時。
    pub modified: std::time::SystemTime,
}

/// アーカイブハンドラのトレイト
/// 各フォーマットのハンドラが実装する
pub trait ArchiveHandler: Send + Sync {
    fn supported_extensions(&self) -> Vec<String>;

    /// アーカイブ内の画像ファイルをtarget_dirに展開する
    /// 戻り値は展開先・元のエントリ名・更新日時の一覧。
    fn extract_images(
        &self,
        _archive_path: &Path,
        _target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        bail!("一括展開未対応")
    }

    fn supports_on_demand(&self) -> bool {
        false
    }

    fn list_images_from_buffer(&self, _buffer: &[u8]) -> Result<Vec<ArchiveImageEntry>> {
        bail!("オンデマンド列挙未対応")
    }

    fn read_entry_at(&self, _path: &Path, _buffer: Option<&[u8]>, _index: u32) -> Result<Vec<u8>> {
        bail!("オンデマンド取得未対応")
    }
}

/// アーカイブハンドラのディスパッチャ
pub struct ArchiveManager {
    handlers: Vec<Box<dyn ArchiveHandler>>,
    registry: Arc<ExtensionRegistry>,
}

impl ArchiveManager {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self {
            handlers: vec![
                Box::new(zip::ZipHandler::new(Arc::clone(&registry))),
                Box::new(rar::RarHandler::new(Arc::clone(&registry))),
                Box::new(sevenz::SevenZHandler::new(Arc::clone(&registry))),
            ],
            registry,
        }
    }

    /// ハンドラを追加する (Susieプラグイン等の動的追加用)
    pub fn add_handler(&mut self, handler: Box<dyn ArchiveHandler>) {
        self.handlers.push(handler);
    }

    /// パスの拡張子を正規化する (例: "test.ZIP" → ".zip")
    fn normalized_extension(path: &Path) -> String {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default()
    }

    /// パスの拡張子に対応するハンドラを返す
    fn find_handler(&self, archive_path: &Path) -> Result<&dyn ArchiveHandler> {
        let ext = Self::normalized_extension(archive_path);
        self.handlers
            .iter()
            .find(|h| h.supported_extensions().contains(&ext))
            .map(AsRef::as_ref)
            .ok_or_else(|| anyhow::anyhow!("未対応のアーカイブ形式: {}", archive_path.display()))
    }

    /// パスがアーカイブファイルか拡張子で判定する
    pub fn is_archive(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| self.registry.is_archive_extension(name))
    }

    /// 対応ハンドラでアーカイブ内の画像を一括展開する
    pub fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        self.find_handler(archive_path)?
            .extract_images(archive_path, target_dir)
    }

    /// パスに対応するハンドラがオンデマンド取得に対応しているか判定する
    pub fn supports_on_demand(&self, archive_path: &Path) -> bool {
        self.find_handler(archive_path)
            .is_ok_and(ArchiveHandler::supports_on_demand)
    }

    /// ハンドラの抽象を通じてバッファから画像を列挙する。
    pub fn list_images_from_buffer(
        &self,
        buffer: &[u8],
        archive_path: &Path,
    ) -> Result<Vec<ArchiveImageEntry>> {
        self.find_handler(archive_path)?
            .list_images_from_buffer(buffer)
    }

    /// ハンドラの抽象を通じて番号指定の内容を取得する。
    pub fn read_entry_at(
        &self,
        archive_path: &Path,
        buffer: Option<&[u8]>,
        index: u32,
    ) -> Result<Vec<u8>> {
        self.find_handler(archive_path)?
            .read_entry_at(archive_path, buffer, index)
    }
}

/// アーカイブ展開時のファイル名重複を解決する
/// target_dirに同名ファイルが既に存在する場合、"_2", "_3"...のサフィックスを付ける
pub fn resolve_filename(target_dir: &Path, original_name: &str) -> std::path::PathBuf {
    let path = target_dir.join(original_name);
    if !path.exists() {
        return path;
    }

    let stem = Path::new(original_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(original_name);
    let ext = Path::new(original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    for i in 2..=9999 {
        let new_name = if ext.is_empty() {
            format!("{stem}_{i}")
        } else {
            format!("{stem}_{i}.{ext}")
        };
        let new_path = target_dir.join(&new_name);
        if !new_path.exists() {
            return new_path;
        }
    }

    // 9999まで全て使用済みの場合 (実質ありえない)
    target_dir.join(format!("{original_name}_overflow"))
}

/// アーカイブエントリのパスからファイル名部分のみを抽出する
/// アーカイブ内のサブフォルダ構造はフラット化する
pub fn extract_filename(entry_path: &str) -> &str {
    // パス区切りは '/' と '\' の両方を考慮
    entry_path.rsplit(['/', '\\']).next().unwrap_or(entry_path)
}

/// 全アーカイブ形式で共通の画像エントリ選別。
pub fn is_image_entry(path: &str, is_directory: bool, registry: &ExtensionRegistry) -> bool {
    let name = extract_filename(path);
    !is_directory && !name.is_empty() && !name.starts_with('.') && registry.is_image_extension(name)
}

/// DOS日時を暦の成分から変換する。タイムゾーン情報は格納されていないためUTCとして扱う。
/// 不正な成分は更新日時なしとしてUNIX epochを返す。
pub fn dos_modified(packed: u32) -> std::time::SystemTime {
    let Ok(time) = ::zip::DateTime::try_from_msdos((packed >> 16) as u16, packed as u16) else {
        return std::time::SystemTime::UNIX_EPOCH;
    };
    let year = time.year();
    let leap = |year: u16| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let mut months = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if leap(year) {
        months[1] = 29;
    }
    if u64::from(time.day()) > months[usize::from(time.month() - 1)] {
        return std::time::SystemTime::UNIX_EPOCH;
    }
    let days = (1970..year)
        .map(|year| if leap(year) { 366u64 } else { 365 })
        .sum::<u64>()
        + months[..usize::from(time.month() - 1)].iter().sum::<u64>()
        + u64::from(time.day() - 1);
    let seconds = days * 86400
        + u64::from(time.hour()) * 3600
        + u64::from(time.minute()) * 60
        + u64::from(time.second());
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_archive_recognizes_extensions() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        assert!(mgr.is_archive(Path::new("test.zip")));
        assert!(mgr.is_archive(Path::new("test.ZIP")));
        assert!(mgr.is_archive(Path::new("test.cbz")));
        assert!(mgr.is_archive(Path::new("test.rar")));
        assert!(mgr.is_archive(Path::new("test.cbr")));
        assert!(mgr.is_archive(Path::new("test.7z")));
        assert!(!mgr.is_archive(Path::new("test.jpg")));
        assert!(!mgr.is_archive(Path::new("test.txt")));
        assert!(!mgr.is_archive(Path::new("test")));
    }

    #[test]
    fn extract_filename_flattens_paths() {
        assert_eq!(extract_filename("folder/subfolder/image.jpg"), "image.jpg");
        assert_eq!(extract_filename("folder\\image.png"), "image.png");
        assert_eq!(extract_filename("image.bmp"), "image.bmp");
        assert_eq!(extract_filename(""), "");
    }

    #[test]
    fn resolve_filename_handles_duplicates() {
        let dir = crate::test_helpers::TempDir::new("resolve_fn");

        // 重複なし
        let path = resolve_filename(&dir, "unique.jpg");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "unique.jpg");

        // 重複あり
        std::fs::write(dir.join("dup.jpg"), b"").unwrap();
        let path = resolve_filename(&dir, "dup.jpg");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "dup_2.jpg");
    }

    #[test]
    fn find_handler_returns_error_for_unsupported_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.find_handler(Path::new("test.xyz"));
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("未対応のアーカイブ形式"), "got: {msg}");
    }

    #[test]
    fn find_handler_returns_error_for_no_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.find_handler(Path::new("noext"));
        assert!(result.is_err());
    }

    #[test]
    fn find_handler_succeeds_for_supported_extensions() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        // 各ハンドラの代表拡張子で成功すること
        assert!(mgr.find_handler(Path::new("a.zip")).is_ok());
        assert!(mgr.find_handler(Path::new("a.cbz")).is_ok());
        assert!(mgr.find_handler(Path::new("a.rar")).is_ok());
        assert!(mgr.find_handler(Path::new("a.cbr")).is_ok());
        assert!(mgr.find_handler(Path::new("a.7z")).is_ok());
        // 大文字小文字の正規化も find_handler 経由で確認 (normalized_extension の置き換え)
        assert!(mgr.find_handler(Path::new("a.ZIP")).is_ok());
        assert!(mgr.find_handler(Path::new("a.Rar")).is_ok());
        // 拡張子なしは失敗
        assert!(mgr.find_handler(Path::new("noext")).is_err());
    }

    // normalized_extension の単体テストは削除済み:
    // 上の find_handler_succeeds_for_supported_extensions が大文字小文字混在の代表的な
    // 入力で結合的にカバーしており、private 関数の単独テストは冗長。

    #[test]
    fn list_images_from_buffer_empty_zip() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        // 空のZIPバッファを作成
        let buf = {
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let writer = ::zip::ZipWriter::new(cursor);
            writer.finish().unwrap();
            v
        };
        let entries = mgr
            .list_images_from_buffer(&buf, Path::new("empty.zip"))
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_images_from_buffer_filters_non_images() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        // 画像と非画像が混在するZIP
        let buf = {
            use std::io::Write;
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let mut writer = ::zip::ZipWriter::new(cursor);
            let opts = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Stored);
            writer.start_file("photo.jpg", opts).unwrap();
            writer.write_all(b"jpeg-data").unwrap();
            writer.start_file("readme.txt", opts).unwrap();
            writer.write_all(b"text").unwrap();
            writer.start_file("doc.pdf", opts).unwrap();
            writer.write_all(b"pdf").unwrap();
            writer.start_file("icon.png", opts).unwrap();
            writer.write_all(b"png-data").unwrap();
            writer.start_file("data.xml", opts).unwrap();
            writer.write_all(b"xml").unwrap();
            writer.finish().unwrap();
            v
        };
        let entries = mgr
            .list_images_from_buffer(&buf, Path::new("mixed.zip"))
            .unwrap();
        // jpg と png のみ通過する
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name, "photo.jpg");
        assert_eq!(entries[1].file_name, "icon.png");
    }

    #[test]
    fn list_images_from_buffer_works_with_cbz_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        let buf = {
            use std::io::Write;
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let mut writer = ::zip::ZipWriter::new(cursor);
            let opts = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Stored);
            writer.start_file("page1.png", opts).unwrap();
            writer.write_all(b"png").unwrap();
            writer.finish().unwrap();
            v
        };
        // .cbzもZIPとして処理される
        let entries = mgr
            .list_images_from_buffer(&buf, Path::new("comic.cbz"))
            .unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn list_images_from_buffer_rejects_non_zip_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.list_images_from_buffer(b"", Path::new("test.rar"));
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("オンデマンド列挙未対応"), "got: {msg}");
    }

    #[test]
    fn supports_on_demand_returns_false_for_unsupported() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        // 未対応拡張子 → find_handlerが失敗 → false
        assert!(!mgr.supports_on_demand(Path::new("test.xyz")));
    }

    #[test]
    fn supports_on_demand_for_zip() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        // ZipHandlerのsupports_on_demandはtrueを返す
        assert!(mgr.supports_on_demand(Path::new("test.zip")));
    }

    #[test]
    fn read_entry_fails_for_unsupported_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.read_entry_at(Path::new("test.xyz"), None, 0);
        assert!(result.is_err());
    }

    #[test]
    fn extract_images_fails_for_unsupported_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.extract_images(Path::new("test.xyz"), Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_filename_no_extension() {
        let dir = crate::test_helpers::TempDir::new("resolve_noext");

        // 拡張子なしファイルの重複解決
        std::fs::write(dir.join("README"), b"").unwrap();
        let path = resolve_filename(&dir, "README");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "README_2");
    }

    #[test]
    fn resolve_filename_multiple_duplicates() {
        let dir = crate::test_helpers::TempDir::new("resolve_multi");

        // 連番の重複解決: _2, _3 と順に増える
        std::fs::write(dir.join("img.png"), b"").unwrap();
        std::fs::write(dir.join("img_2.png"), b"").unwrap();
        let path = resolve_filename(&dir, "img.png");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "img_3.png");
    }

    #[test]
    fn extract_filename_trailing_separator() {
        // 末尾が区切り文字の場合 → 空文字列
        assert_eq!(extract_filename("folder/"), "");
        assert_eq!(extract_filename("folder\\"), "");
    }

    #[test]
    fn is_archive_rejects_path_without_filename() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        // パスの末尾が区切り文字 (file_name() がNone)
        assert!(!mgr.is_archive(Path::new("/")));
    }
}
