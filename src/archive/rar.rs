use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use super::{ArchiveHandler, ExtractedEntry, extract_filename, resolve_filename};
use crate::extension_registry::ExtensionRegistry;

pub(super) const EXTENSIONS: &[&str] = &[".rar", ".cbr"];

pub struct RarHandler {
    registry: Arc<ExtensionRegistry>,
}

impl RarHandler {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self { registry }
    }
}

impl ArchiveHandler for RarHandler {
    fn supported_extensions(&self) -> Vec<String> {
        EXTENSIONS.iter().map(ToString::to_string).collect()
    }

    fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        let mut archive = unrar::Archive::new(archive_path)
            .open_for_processing()
            .map_err(|e| anyhow::anyhow!("RARアーカイブを開けない: {e}"))?;

        let mut results = Vec::new();

        loop {
            let cursor = match archive.read_header() {
                Ok(Some(cursor)) => cursor,
                Ok(None) => break,
                Err(e) => {
                    eprintln!("RARヘッダ読み取り失敗: {e}");
                    break;
                }
            };

            let entry = cursor.entry();
            let entry_path = entry.filename.to_string_lossy().to_string();
            let filename = extract_filename(&entry_path);
            let modified = super::dos_modified(entry.file_time);

            let should_extract =
                super::is_image_entry(&entry_path, entry.is_directory(), &self.registry);

            if should_extract {
                match cursor.read() {
                    Ok((data, next)) => {
                        let out_path = resolve_filename(target_dir, filename);
                        if std::fs::write(&out_path, &data).is_ok() {
                            results.push((out_path, entry_path, modified));
                        }
                        archive = next;
                    }
                    Err(e) => {
                        eprintln!("RARエントリ読み取り失敗: {entry_path}: {e}");
                        break;
                    }
                }
            } else {
                match cursor.skip() {
                    Ok(next) => {
                        archive = next;
                    }
                    Err(e) => {
                        eprintln!("RARエントリスキップ失敗: {entry_path}: {e}");
                        break;
                    }
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_manager_dispatches_rar_extensions_to_rar_handler() {
        let registry = Arc::new(ExtensionRegistry::new());
        let manager = crate::archive::ArchiveManager::new(registry);
        let dir = crate::test_helpers::TempDir::new("rar_dispatch");
        for name in ["absent.rar", "absent.cbr", "absent.cbrr"] {
            let result = manager.extract_images(&dir.join(name), &dir);
            let Err(error) = result else {
                panic!("不在ファイルを展開できるべきではない: {name}");
            };
            let expected = if name == "absent.cbrr" {
                "未対応のアーカイブ形式:"
            } else {
                "RARアーカイブを開けない:"
            };
            assert!(error.to_string().starts_with(expected), "{name}: {error}");
        }
    }

    #[test]
    fn nonexistent_rar_returns_error() {
        let reg = Arc::new(ExtensionRegistry::new());
        let handler = RarHandler::new(reg);
        let dir = crate::test_helpers::TempDir::new("rar_noexist");
        let target = dir.join("extract");
        std::fs::create_dir(&target).unwrap();
        let result = handler.extract_images(&dir.join("absent.rar"), &target);
        assert!(
            matches!(result, Err(error) if error.to_string().starts_with("RARアーカイブを開けない:"))
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }
}
