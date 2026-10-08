use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use super::{ArchiveHandler, ExtractedEntry, extract_filename, resolve_filename};
use crate::extension_registry::ExtensionRegistry;

pub(super) const EXTENSIONS: &[&str] = &[".7z"];

/// 7zアーカイブハンドラ
pub struct SevenZHandler {
    registry: Arc<ExtensionRegistry>,
}

impl SevenZHandler {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self { registry }
    }
}

impl ArchiveHandler for SevenZHandler {
    fn supported_extensions(&self) -> Vec<String> {
        EXTENSIONS.iter().map(ToString::to_string).collect()
    }

    fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        let file = File::open(archive_path)
            .with_context(|| format!("アーカイブを開けない: {}", archive_path.display()))?;

        let mut results: Vec<ExtractedEntry> = Vec::new();
        let target_dir = target_dir.to_path_buf();
        let registry = Arc::clone(&self.registry);

        // ArchiveReaderで各エントリをコールバック処理
        let mut reader = sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty())
            .with_context(|| format!("7zアーカイブ読み取り失敗: {}", archive_path.display()))?;

        reader
            .for_each_entries(|entry, data_reader| {
                let entry_path = entry.name().to_string();
                let filename = extract_filename(&entry_path);

                if !super::is_image_entry(&entry_path, entry.is_directory(), &registry) {
                    return Ok(true);
                }

                // エントリデータを取得
                let mut data = Vec::new();
                std::io::Read::read_to_end(data_reader, &mut data)?;

                // target_dirに保存
                let out_path = resolve_filename(&target_dir, filename);
                std::fs::write(&out_path, &data)?;
                results.push((
                    out_path,
                    entry_path,
                    if entry.has_last_modified_date {
                        entry.last_modified_date().into()
                    } else {
                        std::time::SystemTime::UNIX_EPOCH
                    },
                ));

                Ok(true)
            })
            .with_context(|| format!("7zアーカイブ展開失敗: {}", archive_path.display()))?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions() {
        let reg = Arc::new(ExtensionRegistry::new());
        let handler = SevenZHandler::new(reg);
        assert!(handler.supported_extensions().contains(&".7z".to_string()));
    }

    #[test]
    fn nonexistent_7z_returns_error() {
        let reg = Arc::new(ExtensionRegistry::new());
        let handler = SevenZHandler::new(reg);
        let dir = crate::test_helpers::TempDir::new("7z_noexist");
        let result: Result<Vec<super::ExtractedEntry>> =
            handler.extract_images(Path::new("nonexistent.7z"), &dir);
        assert!(result.is_err());
    }
}
