/// Susieプラグインシステム
/// 64bit Susieプラグイン (.sph / .spi) の検出・ロード・管理
mod ffi;
pub mod plugin;
pub mod util;

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::archive::ArchiveHandler;
use crate::extension_registry::ExtensionRegistry;
use crate::image::ImageDecoder;

use plugin::{SharedPlugin, SusiePlugin, SusiePluginType};

/// Susieプラグインマネージャ
/// spiディレクトリからプラグインを検出・ロードする
#[derive(Default)]
pub struct SusieManager {
    plugins: Vec<SharedPlugin>,
}

impl SusieManager {
    /// spiディレクトリからプラグインを自動検出・ロードする
    /// ロード失敗は警告表示してスキップ（アプリ起動は止めない）
    pub fn discover(spi_dir: &Path) -> Self {
        let mut plugins = Vec::new();

        if !spi_dir.is_dir() {
            return Self { plugins };
        }

        let entries = match std::fs::read_dir(spi_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("SPIディレクトリ読み取り失敗: {}: {e}", spi_dir.display());
                return Self { plugins };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // .sph / .spi 拡張子のみ対象
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            if !matches!(ext.as_deref(), Some("sph") | Some("spi")) {
                continue;
            }

            match SusiePlugin::try_load(&path) {
                Ok(plugin) => {
                    let exts = plugin.supported_extensions();
                    eprintln!(
                        "Susieプラグインロード: {} ({:?}, 拡張子: {:?})",
                        path.display(),
                        plugin.plugin_type,
                        exts
                    );
                    plugins.push(Arc::new(Mutex::new(plugin)));
                }
                Err(e) => {
                    eprintln!("Susieプラグインロード失敗: {}: {e}", path.display());
                }
            }
        }

        Self { plugins }
    }

    /// プラグインが報告する拡張子をExtensionRegistryに登録する
    pub fn register_extensions(&self, registry: &mut ExtensionRegistry) {
        for plugin in &self.plugins {
            let locked = plugin.lock().expect("Susie plugin lock poisoned");
            let exts = locked.supported_extensions();
            match locked.plugin_type {
                SusiePluginType::Image => {
                    registry.register_image_extensions(&exts);
                }
                SusiePluginType::Archive => {
                    registry.register_archive_extensions(&exts);
                }
            }
        }
    }

    /// 画像プラグインからImageDecoderアダプタを生成する
    pub fn create_image_decoders(&self) -> Vec<Box<dyn ImageDecoder>> {
        self.plugins
            .iter()
            .filter(|p| {
                p.lock().expect("Susie plugin lock poisoned").plugin_type == SusiePluginType::Image
            })
            .map(|p| {
                Box::new(super::image::susie::SusieImageDecoder::new(Arc::clone(p)))
                    as Box<dyn ImageDecoder>
            })
            .collect()
    }

    /// アーカイブプラグインからArchiveHandlerアダプタを生成する
    pub fn create_archive_handlers(
        &self,
        registry: Arc<ExtensionRegistry>,
    ) -> Vec<Box<dyn ArchiveHandler>> {
        self.plugins
            .iter()
            .filter(|p| {
                p.lock().expect("Susie plugin lock poisoned").plugin_type
                    == SusiePluginType::Archive
            })
            .map(|p| {
                Box::new(super::archive::susie::SusieArchiveHandler::new(
                    Arc::clone(p),
                    Arc::clone(&registry),
                )) as Box<dyn ArchiveHandler>
            })
            .collect()
    }
}
