/// Susie画像プラグインのImageDecoderアダプタ
use anyhow::Result;

use crate::susie::plugin::SharedPlugin;

use super::{DecodedImage, ImageDecoder, ImageMetadata, read_exif_fields};

/// Susie画像プラグインをImageDecoderとして使うアダプタ
pub struct SusieImageDecoder {
    plugin: SharedPlugin,
}

impl SusieImageDecoder {
    pub fn new(plugin: SharedPlugin) -> Self {
        Self { plugin }
    }
}

impl ImageDecoder for SusieImageDecoder {
    fn can_decode(&self, data: &[u8], filename_hint: &str) -> bool {
        let locked = self.plugin.lock().expect("Susie plugin lock poisoned");
        locked.is_supported(filename_hint, data)
    }

    fn decode(&self, data: &[u8], filename_hint: &str) -> Result<DecodedImage> {
        let locked = self.plugin.lock().expect("Susie plugin lock poisoned");
        locked.get_picture(data, filename_hint)
    }

    fn metadata(&self, data: &[u8], _filename_hint: &str) -> Result<ImageMetadata> {
        // EXIFメタデータ (Susie経由でもraw bytesからEXIFを読み取れる)
        let exif = read_exif_fields(data);
        let plugin_path = self
            .plugin
            .lock()
            .expect("Susie plugin lock poisoned")
            .path
            .display()
            .to_string();
        Ok(ImageMetadata {
            format: format!("Susie ({plugin_path})"),
            comments: Vec::new(),
            exif,
        })
    }
}
