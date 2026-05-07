use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{ArchiveHandler, ExtractedEntry, extract_filename, resolve_filename};
use crate::extension_registry::ExtensionRegistry;

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

    /// インメモリバッファから名前指定でエントリを取得する (旧形式ブックマーク互換用)
    ///
    /// zipクレートv8の`by_name`は内部キーとして原バイト列を使うため、
    /// CP932で記録された日本語ファイル名にはマッチしない。
    /// 新規呼び出しは`read_entry_from_buffer_at`を使うこと。
    pub(crate) fn read_entry_from_buffer(buffer: &[u8], entry_name: &str) -> Result<Vec<u8>> {
        let cursor = std::io::Cursor::new(buffer);
        let mut archive = zip::ZipArchive::new(cursor).context("ZIPバッファの読み取りに失敗")?;
        let mut entry = archive
            .by_name(entry_name)
            .with_context(|| format!("エントリが見つからない: {entry_name}"))?;

        if entry.compression() == zip::CompressionMethod::Stored {
            let start = entry.data_start().context("データ開始位置の取得に失敗")? as usize;
            let size = entry.size() as usize;
            drop(entry);
            return Ok(buffer[start..start + size].to_vec());
        }

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
            if entry.is_dir() {
                continue;
            }
            let entry_name = decode_zip_entry_name(entry.name_raw());
            let file_size = entry.size();
            let filename = extract_filename(&entry_name).to_string();
            if filename.is_empty() || filename.starts_with('.') {
                continue;
            }
            if !registry.is_image_extension(&filename) {
                continue;
            }
            results.push(super::ArchiveImageEntry {
                entry_name,
                file_name: filename,
                file_size,
                entry_index: i as u32,
            });
        }
        results
    }
}

impl ArchiveHandler for ZipHandler {
    fn supported_extensions(&self) -> Vec<String> {
        vec![".zip".to_string(), ".cbz".to_string()]
    }

    fn supports_on_demand(&self) -> bool {
        true
    }

    fn read_entry(&self, archive_path: &Path, entry_name: &str) -> Result<Vec<u8>> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けない: {}", archive_path.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("ZIP読み取り失敗: {}", archive_path.display()))?;
        let mut entry = archive
            .by_name(entry_name)
            .with_context(|| format!("エントリが見つからない: {entry_name}"))?;
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data)?;
        Ok(data)
    }

    fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けない: {}", archive_path.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("ZIP読み取り失敗: {}", archive_path.display()))?;

        let mut results = Vec::new();

        for i in 0..archive.len() {
            let Ok(mut entry) = archive.by_index(i) else {
                continue;
            };

            // ディレクトリエントリはスキップ
            if entry.is_dir() {
                continue;
            }

            let entry_name = decode_zip_entry_name(entry.name_raw());
            let filename = extract_filename(&entry_name).to_string();

            // 空ファイル名やドット始まりの隠しファイルはスキップ
            if filename.is_empty() || filename.starts_with('.') {
                continue;
            }

            // 画像ファイルのみ展開
            if !self.registry.is_image_extension(&filename) {
                continue;
            }

            // ファイルデータを取得
            let mut data = Vec::new();
            if entry.read_to_end(&mut data).is_err() {
                continue;
            }

            // target_dirに保存 (重複時はリネーム)
            let out_path = resolve_filename(target_dir, &filename);
            if std::fs::write(&out_path, &data).is_ok() {
                results.push((out_path, entry_name));
            }
        }

        Ok(results)
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
    use std::io::Write;

    /// テスト用ZIPをメモリ上で作成してtempに保存する
    fn create_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (name, data) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extract_images_from_zip() {
        let dir = std::env::temp_dir().join("gv_test_zip_extract");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("test.zip");
        create_test_zip(
            &zip_path,
            &[
                ("image1.jpg", b"fake-jpg-data"),
                ("subfolder/image2.png", b"fake-png-data"),
                ("readme.txt", b"not an image"),
                ("image3.bmp", b"fake-bmp-data"),
            ],
        );

        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let entries = handler.extract_images(&zip_path, &out_dir).unwrap();

        assert_eq!(entries.len(), 3);
        assert!(out_dir.join("image1.jpg").exists());
        assert!(out_dir.join("image2.png").exists()); // サブフォルダはフラット化
        assert!(out_dir.join("image3.bmp").exists());
        assert!(!out_dir.join("readme.txt").exists());

        // エントリパスが元のアーカイブ内パスを保持していることを確認
        let entry_paths: Vec<&str> = entries.iter().map(|(_, e)| e.as_str()).collect();
        assert!(entry_paths.contains(&"image1.jpg"));
        assert!(entry_paths.contains(&"subfolder/image2.png"));
        assert!(entry_paths.contains(&"image3.bmp"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_handles_duplicate_filenames() {
        let dir = std::env::temp_dir().join("gv_test_zip_dup");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("dup.zip");
        create_test_zip(
            &zip_path,
            &[("a/image.jpg", b"data1"), ("b/image.jpg", b"data2")],
        );

        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let entries = handler.extract_images(&zip_path, &out_dir).unwrap();

        assert_eq!(entries.len(), 2);
        assert!(out_dir.join("image.jpg").exists());
        assert!(out_dir.join("image_2.jpg").exists());

        // 元エントリパスはそれぞれ異なる
        assert_eq!(entries[0].1, "a/image.jpg");
        assert_eq!(entries[1].1, "b/image.jpg");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ZIPをメモリ上で作成しバイト列として返す
    fn create_test_zip_buffer(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn list_images_returns_image_entries_only() {
        let dir = std::env::temp_dir().join("gv_test_zip_list");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_entry_returns_data() {
        let dir = std::env::temp_dir().join("gv_test_zip_read");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("test.zip");
        create_test_zip(&zip_path, &[("image.jpg", b"fake-jpg-data-123")]);

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let data = handler.read_entry(&zip_path, "image.jpg").unwrap();
        assert_eq!(data, b"fake-jpg-data-123");

        let _ = std::fs::remove_dir_all(&dir);
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
    fn read_entry_from_buffer_legacy_name_lookup() {
        // pub(crate) APIで名前ベースのlookupが従来通り動くことを確認 (旧ブックマーク互換用)
        let buffer = create_test_zip_buffer(&[("img.jpg", b"legacy-data")]);
        let data = ZipHandler::read_entry_from_buffer(&buffer, "img.jpg").unwrap();
        assert_eq!(data, b"legacy-data");
    }

    // --- decode_zip_entry_name テスト ---

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

    /// ファイル名フィールドにCP932バイト列を直接書き込んだ最小ZIPバイナリを組み立てる
    ///
    /// 目的: zipクレートの`ZipWriter`はファイル名をUTF-8で書き込み汎用ビットフラグの
    /// 第11ビット (UTF-8) を立てるため、後処理ではCP932記録ZIPを再現できない。
    /// 本関数はZIP仕様 (APPNOTE 4.3) に従い、ローカルファイルヘッダー・セントラル
    /// ディレクトリ・EOCDの長さとオフセットを整合させた上で、ファイル名フィールドへ
    /// CP932バイト列をそのまま書き込む。
    ///
    /// データはStored (無圧縮) で書き込み、CRC32は標準的なIEEEテーブルで計算する。
    fn build_cp932_zip(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut central_dir_records = Vec::new();
        let mut central_dir_offset_per_entry = Vec::new();

        for (name, data) in entries {
            let crc = crc32(data);
            let local_header_offset = buf.len() as u32;
            central_dir_offset_per_entry.push(local_header_offset);

            // Local file header (signature 0x04034b50)
            buf.extend_from_slice(&0x04034b50u32.to_le_bytes());
            buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
            buf.extend_from_slice(&0u16.to_le_bytes()); // general purpose bit flag (UTF-8 ビット非設定)
            buf.extend_from_slice(&0u16.to_le_bytes()); // compression: stored
            buf.extend_from_slice(&0u16.to_le_bytes()); // mod time
            buf.extend_from_slice(&0u16.to_le_bytes()); // mod date
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes()); // compressed size
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes()); // uncompressed size
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // extra field length
            buf.extend_from_slice(name);
            buf.extend_from_slice(data);

            // Central directory record (構築は後回し、各エントリのバイト列だけ作る)
            let mut cd = Vec::new();
            cd.extend_from_slice(&0x02014b50u32.to_le_bytes()); // signature
            cd.extend_from_slice(&20u16.to_le_bytes()); // version made by
            cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
            cd.extend_from_slice(&0u16.to_le_bytes()); // general purpose bit flag
            cd.extend_from_slice(&0u16.to_le_bytes()); // compression
            cd.extend_from_slice(&0u16.to_le_bytes()); // mod time
            cd.extend_from_slice(&0u16.to_le_bytes()); // mod date
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // extra field length
            cd.extend_from_slice(&0u16.to_le_bytes()); // file comment length
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal file attributes
            cd.extend_from_slice(&0u32.to_le_bytes()); // external file attributes
            cd.extend_from_slice(&local_header_offset.to_le_bytes());
            cd.extend_from_slice(name);
            central_dir_records.push(cd);
        }

        let central_dir_start = buf.len() as u32;
        let mut central_dir_size: u32 = 0;
        for cd in &central_dir_records {
            buf.extend_from_slice(cd);
            central_dir_size += cd.len() as u32;
        }

        // End of central directory record (signature 0x06054b50)
        buf.extend_from_slice(&0x06054b50u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk number
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk where central dir starts
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // central dir records on this disk
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // total central dir records
        buf.extend_from_slice(&central_dir_size.to_le_bytes());
        buf.extend_from_slice(&central_dir_start.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // comment length
        buf
    }

    /// IEEE 802.3 CRC32 (zip仕様)。標準テーブル方式で1バイトずつ計算する
    fn crc32(data: &[u8]) -> u32 {
        const POLY: u32 = 0xEDB88320;
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { POLY ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }

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

        // 読み出し: インデックスベースで成功すること
        let data = ZipHandler::read_entry_from_buffer_at(&buffer, 0).unwrap();
        assert_eq!(data, payload);

        // 名前ベースの旧APIでは復号後の文字列ではマッチしないことも確認 (再lookupが失敗する仕様)
        let result = ZipHandler::read_entry_from_buffer(&buffer, "画像.jpg");
        assert!(result.is_err(), "復号後の名前で再lookupは失敗するべき");
    }

    #[test]
    fn cp932_filename_zip_extract_images_decodes_name() {
        let cp932_name: &[u8] = &[0x89, 0xE6, 0x91, 0x9C, b'.', b'j', b'p', b'g'];
        let payload = b"image-data";
        let buffer = build_cp932_zip(&[(cp932_name, payload)]);

        let dir = std::env::temp_dir().join("gv_test_zip_cp932_extract");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("cp932.zip");
        std::fs::write(&zip_path, &buffer).unwrap();
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let reg = Arc::new(ExtensionRegistry::new());
        let handler = ZipHandler::new(reg);
        let entries = handler.extract_images(&zip_path, &out_dir).unwrap();

        assert_eq!(entries.len(), 1);
        // 展開先ファイル名が日本語として正しいこと
        assert!(out_dir.join("画像.jpg").exists());
        // エントリ名 (展開先と元名の対応) も日本語に復号されていること
        assert_eq!(entries[0].1, "画像.jpg");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
