/// Susieアーカイブプラグインの ArchiveHandler アダプタ
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use super::{ArchiveHandler, ExtractedEntry, extract_filename, resolve_filename};
use crate::extension_registry::ExtensionRegistry;
use crate::susie::plugin::SharedPlugin;
use crate::susie::util::from_ansi;

/// SusieアーカイブプラグインをArchiveHandlerとして使うアダプタ
pub struct SusieArchiveHandler {
    plugin: SharedPlugin,
    registry: Arc<ExtensionRegistry>,
    /// キャッシュした拡張子リスト
    extensions: Vec<String>,
}

impl SusieArchiveHandler {
    pub fn new(plugin: SharedPlugin, registry: Arc<ExtensionRegistry>) -> Self {
        let extensions = plugin
            .lock()
            .expect("Susie plugin lock poisoned")
            .supported_extensions();
        Self {
            plugin,
            registry,
            extensions,
        }
    }
}

impl ArchiveHandler for SusieArchiveHandler {
    fn supported_extensions(&self) -> Vec<String> {
        self.extensions.clone()
    }

    fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        let path_str = archive_path.to_string_lossy().to_string();
        let locked = self.plugin.lock().expect("Susie plugin lock poisoned");

        // アーカイブ内のエントリ一覧を取得
        let entries = locked.get_archive_info(&path_str)?;

        let mut results = Vec::new();

        for entry in &entries {
            // ファイル名を取得 (ANSI → UTF-8)
            let raw_filename = from_ansi(&entry.filename);
            let filename = extract_filename(&raw_filename);

            if !super::is_image_entry(&raw_filename, false, &self.registry) {
                continue;
            }

            // メモリに展開
            let position = { entry.position };
            match locked.get_file_to_memory(&path_str, position) {
                Ok(data) => {
                    let out_path = resolve_filename(target_dir, filename);
                    if std::fs::write(&out_path, &data).is_ok() {
                        let timestamp = { entry.timestamp };
                        let duration = std::time::Duration::from_secs(timestamp.unsigned_abs());
                        let modified = if timestamp >= 0 {
                            std::time::SystemTime::UNIX_EPOCH.checked_add(duration)
                        } else {
                            std::time::SystemTime::UNIX_EPOCH.checked_sub(duration)
                        }
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        results.push((out_path, raw_filename, modified));
                    }
                }
                Err(e) => {
                    eprintln!("Susieアーカイブエントリ展開失敗: {filename}: {e}");
                }
            }
        }

        Ok(results)
    }
}
