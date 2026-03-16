use std::fs::File;
use std::path::Path;

use anyhow::{Context as _, Result};

use super::{ArchiveHandler, extract_filename, resolve_filename};
use crate::file_list::is_image_extension;

/// 7zアーカイブハンドラ
pub struct SevenZHandler;

impl ArchiveHandler for SevenZHandler {
    fn supported_extensions(&self) -> &[&str] {
        &[".7z"]
    }

    fn extract_images(&self, archive_path: &Path, target_dir: &Path) -> Result<usize> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けません: {}", archive_path.display()))?;
        let len = file.metadata()?.len();

        let mut count = 0;
        let target_dir = target_dir.to_path_buf();

        // SevenZReaderで各エントリをコールバック処理
        let mut reader = sevenz_rust::SevenZReader::new(file, len, sevenz_rust::Password::empty())
            .with_context(|| format!("7zアーカイブ読み取り失敗: {}", archive_path.display()))?;

        reader
            .for_each_entries(|entry, data_reader| {
                let entry_path = entry.name();
                let filename = extract_filename(entry_path);

                // ディレクトリ、空ファイル名、隠しファイルはスキップ
                if entry.is_directory() || filename.is_empty() || filename.starts_with('.') {
                    return Ok(true);
                }

                // 画像ファイルのみ展開
                if !is_image_extension(filename) {
                    return Ok(true);
                }

                // エントリデータを読み出し
                let mut data = Vec::new();
                std::io::Read::read_to_end(data_reader, &mut data)?;

                // target_dirに書き出し
                let out_path = resolve_filename(&target_dir, filename);
                std::fs::write(&out_path, &data)?;
                count += 1;

                Ok(true)
            })
            .with_context(|| format!("7zアーカイブ展開失敗: {}", archive_path.display()))?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions() {
        let handler = SevenZHandler;
        assert!(handler.supported_extensions().contains(&".7z"));
    }

    #[test]
    fn nonexistent_7z_returns_error() {
        let handler = SevenZHandler;
        let dir = std::env::temp_dir().join("gv3_test_7z_noexist");
        let _ = std::fs::create_dir_all(&dir);
        let result = handler.extract_images(Path::new("nonexistent.7z"), &dir);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
