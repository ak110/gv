use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{ArchiveHandler, extract_filename, resolve_filename};
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

impl ArchiveHandler for ZipHandler {
    fn supported_extensions(&self) -> Vec<String> {
        vec![".zip".to_string(), ".cbz".to_string()]
    }

    fn extract_images(&self, archive_path: &Path, target_dir: &Path) -> Result<usize> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けません: {}", archive_path.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("ZIP読み取り失敗: {}", archive_path.display()))?;

        let mut count = 0;

        for i in 0..archive.len() {
            let mut entry = match archive.by_index(i) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // ディレクトリエントリはスキップ
            if entry.is_dir() {
                continue;
            }

            let entry_name = entry.name().to_string();
            let filename = extract_filename(&entry_name);

            // 空ファイル名やドット始まりの隠しファイルはスキップ
            if filename.is_empty() || filename.starts_with('.') {
                continue;
            }

            // 画像ファイルのみ展開
            if !self.registry.is_image_extension(filename) {
                continue;
            }

            // ファイルデータを読み出し
            let mut data = Vec::new();
            if entry.read_to_end(&mut data).is_err() {
                continue;
            }

            // target_dirに書き出し（重複時はリネーム）
            let out_path = resolve_filename(target_dir, filename);
            if std::fs::write(&out_path, &data).is_ok() {
                count += 1;
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// テスト用ZIPをメモリ上で作成してtempに書き出す
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
        let dir = std::env::temp_dir().join("gv3_test_zip_extract");
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
        let count = handler.extract_images(&zip_path, &out_dir).unwrap();

        assert_eq!(count, 3);
        assert!(out_dir.join("image1.jpg").exists());
        assert!(out_dir.join("image2.png").exists()); // サブフォルダはフラット化
        assert!(out_dir.join("image3.bmp").exists());
        assert!(!out_dir.join("readme.txt").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_handles_duplicate_filenames() {
        let dir = std::env::temp_dir().join("gv3_test_zip_dup");
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
        let count = handler.extract_images(&zip_path, &out_dir).unwrap();

        assert_eq!(count, 2);
        assert!(out_dir.join("image.jpg").exists());
        assert!(out_dir.join("image_2.jpg").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
