use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{ArchiveHandler, extract_filename};
use crate::extension_registry::ExtensionRegistry;

pub(super) const EXTENSIONS: &[&str] = &[".zip", ".cbz"];

/// ZIP/cbzアーカイブハンドラ
pub struct ZipHandler {
    registry: Arc<ExtensionRegistry>,
}

impl ZipHandler {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self { registry }
    }
}

impl ZipHandler {
    /// インメモリバッファからエントリ一覧を取得する
    pub fn list_images_from_buffer(
        buffer: &[u8],
        registry: &ExtensionRegistry,
    ) -> Result<Vec<super::ArchiveImageEntry>> {
        let cursor = std::io::Cursor::new(buffer);
        let archive = zip::ZipArchive::new(cursor).context("ZIPバッファの読み取りに失敗")?;
        Ok(Self::list_images_from_archive(archive, registry))
    }

    /// インメモリバッファからインデックス指定でエントリを取得する (Stored最適化付き)
    pub fn read_entry_from_buffer_at(buffer: &[u8], index: u32) -> Result<Vec<u8>> {
        let cursor = std::io::Cursor::new(buffer);
        let mut archive = zip::ZipArchive::new(cursor).context("ZIPバッファの読み取りに失敗")?;
        let mut entry = archive
            .by_index(index as usize)
            .with_context(|| format!("エントリが見つからない: index={index}"))?;

        // Storedエントリ: バッファから直接スライス (zip Readerのオーバーヘッド回避)
        if entry.compression() == zip::CompressionMethod::Stored {
            let start = entry.data_start().context("データ開始位置の取得に失敗")? as usize;
            let size = entry.size() as usize;
            drop(entry);
            return Ok(buffer[start..start + size].to_vec());
        }

        // 圧縮エントリ: 通常の展開
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data)?;
        Ok(data)
    }

    /// ファイルパスからインデックス指定でエントリを取得する
    pub fn read_entry_at(archive_path: &Path, index: u32) -> Result<Vec<u8>> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けない: {}", archive_path.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("ZIP読み取り失敗: {}", archive_path.display()))?;
        let mut entry = archive
            .by_index(index as usize)
            .with_context(|| format!("エントリが見つからない: index={index}"))?;
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data)?;
        Ok(data)
    }

    /// ファイルパスからエントリ一覧を取得する
    #[cfg(test)]
    pub fn list_images(&self, archive_path: &Path) -> Result<Vec<super::ArchiveImageEntry>> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けない: {}", archive_path.display()))?;
        let archive = zip::ZipArchive::new(file)
            .with_context(|| format!("ZIP読み取り失敗: {}", archive_path.display()))?;
        Ok(Self::list_images_from_archive(archive, &self.registry))
    }

    /// ZipArchiveからエントリ一覧を取得する共通実装
    fn list_images_from_archive<R: std::io::Read + std::io::Seek>(
        mut archive: zip::ZipArchive<R>,
        registry: &ExtensionRegistry,
    ) -> Vec<super::ArchiveImageEntry> {
        let mut results = Vec::new();
        for i in 0..archive.len() {
            let Ok(entry) = archive.by_index_raw(i) else {
                continue;
            };
            let entry_name = decode_zip_entry_name(entry.name_raw());
            let file_size = entry.size();
            let filename = extract_filename(&entry_name).to_string();
            if !super::is_image_entry(&entry_name, entry.is_dir(), registry) {
                continue;
            }
            results.push(super::ArchiveImageEntry {
                entry_name,
                file_name: filename,
                file_size,
                entry_index: i as u32,
                modified: entry
                    .last_modified()
                    .map_or(std::time::SystemTime::UNIX_EPOCH, |time| {
                        super::dos_modified(
                            (u32::from(time.datepart()) << 16) | u32::from(time.timepart()),
                        )
                    }),
            });
        }
        results
    }
}

impl ArchiveHandler for ZipHandler {
    fn supported_extensions(&self) -> Vec<String> {
        EXTENSIONS.iter().map(ToString::to_string).collect()
    }

    fn supports_on_demand(&self) -> bool {
        true
    }

    fn list_images_from_buffer(&self, buffer: &[u8]) -> Result<Vec<super::ArchiveImageEntry>> {
        Self::list_images_from_buffer(buffer, &self.registry)
    }

    fn read_entry_at(&self, path: &Path, buffer: Option<&[u8]>, index: u32) -> Result<Vec<u8>> {
        match buffer {
            Some(buffer) => Self::read_entry_from_buffer_at(buffer, index),
            None => Self::read_entry_at(path, index),
        }
    }
}

/// ZIPエントリのファイル名バイト列を表示用文字列にデコードする。
///
/// zipクレートv8の`name()`はファイル名をUTF-8として解釈し、
/// 失敗時にCP437へフォールバックするためCP932記録のZIPで文字化けする。
/// 本関数はUTF-8として有効なら採用し、無効なら`MultiByteToWideChar`に
/// コードページ932 (`CP_SHIFT_JIS`相当) を指定して復号する。
fn decode_zip_entry_name(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    decode_cp932(raw).unwrap_or_else(|| String::from_utf8_lossy(raw).into_owned())
}

/// CP932 (Shift_JIS) バイト列をUTF-8文字列へデコードする。
///
/// `MultiByteToWideChar`にコードページ932を固定指定するため、
/// プロセスの`CP_ACP`設定や言語設定に依存しない。
/// 入力が空、またはWindows API呼び出しが失敗した場合は`None`を返す。
fn decode_cp932(bytes: &[u8]) -> Option<String> {
    use windows::Win32::Globalization::{MB_PRECOMPOSED, MultiByteToWideChar};

    if bytes.is_empty() {
        return Some(String::new());
    }

    const CP932: u32 = 932;

    // SAFETY: Win32 MultiByteToWideChar の規定通り、まず wlen 取得呼び出しで必要ワイド文字数を
    // 確認し、その分の Vec<u16> を渡してデコードする。CP932 + MB_PRECOMPOSED は CP_ACP と同じ
    // 呼び出し形式であり、susie::util::from_ansi と同じパターン。
    unsafe {
        let wlen = MultiByteToWideChar(CP932, MB_PRECOMPOSED, bytes, None);
        if wlen <= 0 {
            return None;
        }

        let mut wide = vec![0u16; wlen as usize];
        let written = MultiByteToWideChar(CP932, MB_PRECOMPOSED, bytes, Some(&mut wide));
        if written <= 0 {
            return None;
        }

        Some(String::from_utf16_lossy(&wide[..written as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{build_cp932_zip, create_test_zip, create_test_zip_buffer};
    use std::io::Write;

    #[test]
    fn list_images_returns_image_entries_only() {
        let dir = crate::test_helpers::TempDir::new("zip_list");
        let zip_path = dir.join("test.zip");
        create_test_zip(
            &zip_path,
            &[
                ("image1.jpg", b"fake-jpg"),
                ("subfolder/image2.png", b"fake-png"),
                ("readme.txt", b"text"),
            ],
        );

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let entries = handler.list_images(&zip_path).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry_name, "image1.jpg");
        assert_eq!(entries[0].file_name, "image1.jpg");
        assert_eq!(entries[0].entry_index, 0);
        assert_eq!(entries[1].entry_name, "subfolder/image2.png");
        assert_eq!(entries[1].file_name, "image2.png");
        assert_eq!(entries[1].entry_index, 1);
    }

    #[test]
    fn read_entry_returns_data() {
        let dir = crate::test_helpers::TempDir::new("zip_read");
        let zip_path = dir.join("test.zip");
        create_test_zip(&zip_path, &[("image.jpg", b"fake-jpg-data-123")]);

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let data = handler.read_entry_at(&zip_path, None, 0).unwrap();
        assert_eq!(data, b"fake-jpg-data-123");
    }

    #[test]
    fn list_images_from_buffer_works() {
        let reg = Arc::new(ExtensionRegistry::new());
        let buffer = create_test_zip_buffer(&[
            ("a.jpg", b"jpg-data"),
            ("b.png", b"png-data"),
            ("c.txt", b"text"),
        ]);
        let entries = ZipHandler::list_images_from_buffer(&buffer, &reg).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name, "a.jpg");
        assert_eq!(entries[0].entry_index, 0);
        assert_eq!(entries[1].file_name, "b.png");
        assert_eq!(entries[1].entry_index, 1);
    }

    #[test]
    fn read_entry_from_buffer_at_stored() {
        let buffer = create_test_zip_buffer(&[("img.jpg", b"stored-data")]);
        let data = ZipHandler::read_entry_from_buffer_at(&buffer, 0).unwrap();
        assert_eq!(data, b"stored-data");
    }

    #[test]
    fn read_entry_from_buffer_at_compressed() {
        // Deflate圧縮のZIPを作成
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("img.jpg", options).unwrap();
            writer.write_all(b"deflated-data-content").unwrap();
            writer.finish().unwrap();
        }
        let data = ZipHandler::read_entry_from_buffer_at(&buf, 0).unwrap();
        assert_eq!(data, b"deflated-data-content");
    }

    #[test]
    fn decode_ascii_name() {
        assert_eq!(decode_zip_entry_name(b"image.jpg"), "image.jpg");
    }

    #[test]
    fn decode_utf8_name() {
        let utf8 = "画像.jpg".as_bytes();
        assert_eq!(decode_zip_entry_name(utf8), "画像.jpg");
    }

    #[test]
    fn decode_cp932_name() {
        // 「画像」のCP932バイト列: 0x89 0xE6 0x91 0x9C
        let cp932: &[u8] = &[0x89, 0xE6, 0x91, 0x9C, b'.', b'j', b'p', b'g'];
        assert_eq!(decode_zip_entry_name(cp932), "画像.jpg");
    }

    #[test]
    fn decode_invalid_bytes_falls_back_to_lossy() {
        // CP932として無効なバイト列の単独使用 (例: 0xFF) は
        // MultiByteToWideCharがエラーになるか置換文字を返す。
        // どちらでもパニックせず文字列を返すことを確認する。
        let bytes: &[u8] = &[0xFF, 0xFE, 0xFD];
        let s = decode_zip_entry_name(bytes);
        assert!(!s.is_empty());
    }

    // --- CP932 ファイル名ZIPの再現テスト ---

    #[test]
    fn cp932_filename_zip_lists_and_reads_correctly() {
        // 「画像.jpg」のCP932バイト列
        let cp932_name: &[u8] = &[0x89, 0xE6, 0x91, 0x9C, b'.', b'j', b'p', b'g'];
        let payload = b"jpeg-bytes-12345";
        let buffer = build_cp932_zip(&[(cp932_name, payload)]);

        // 列挙: name_rawから復号した文字列が日本語として正しいこと
        let reg = Arc::new(ExtensionRegistry::new());
        let entries = ZipHandler::list_images_from_buffer(&buffer, &reg).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_name, "画像.jpg");
        assert_eq!(entries[0].file_name, "画像.jpg");
        assert_eq!(entries[0].entry_index, 0);

        // 取得: インデックスベースで成功すること
        let data = ZipHandler::read_entry_from_buffer_at(&buffer, 0).unwrap();
        assert_eq!(data, payload);
    }
}
