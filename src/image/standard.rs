use anyhow::Context as _;

use super::{DecodedImage, ImageDecoder, ImageMetadata};

/// image crateによる標準デコーダ（JPEG/PNG/GIF/BMP/WebP）
pub struct StandardDecoder;

impl StandardDecoder {
    pub fn new() -> Self {
        Self
    }
}

impl ImageDecoder for StandardDecoder {
    fn supported_extensions(&self) -> &[&str] {
        &[".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp"]
    }

    fn can_decode(&self, data: &[u8]) -> bool {
        image::guess_format(data).is_ok()
    }

    fn decode(&self, data: &[u8]) -> anyhow::Result<DecodedImage> {
        let img = image::load_from_memory(data).context("画像のデコードに失敗")?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();

        Ok(DecodedImage {
            data: rgba.into_raw(),
            width,
            height,
        })
    }

    fn metadata(&self, data: &[u8]) -> anyhow::Result<ImageMetadata> {
        let format = image::guess_format(data)
            .map(|f| format!("{:?}", f))
            .unwrap_or_else(|_| "Unknown".to_string());

        // サイズ取得のためにデコード（ヘッダだけ読むAPIが限定的なため）
        let img = image::load_from_memory(data).context("メタデータの取得に失敗")?;

        Ok(ImageMetadata {
            width: img.width(),
            height: img.height(),
            format,
            comments: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_include_common_formats() {
        let decoder = StandardDecoder::new();
        let exts = decoder.supported_extensions();
        assert!(exts.contains(&".jpg"));
        assert!(exts.contains(&".png"));
        assert!(exts.contains(&".gif"));
        assert!(exts.contains(&".bmp"));
        assert!(exts.contains(&".webp"));
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let decoder = StandardDecoder::new();
        let result = decoder.decode(&[0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn can_decode_rejects_invalid_data() {
        let decoder = StandardDecoder::new();
        assert!(!decoder.can_decode(&[0, 1, 2, 3]));
    }

    #[test]
    fn decode_minimal_png() {
        // 1x1 白ピクセルのPNG
        let png_data = create_1x1_white_png();
        let decoder = StandardDecoder::new();
        assert!(decoder.can_decode(&png_data));

        let img = decoder.decode(&png_data).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        assert_eq!(img.data.len(), 4); // 1 pixel × RGBA
        assert_eq!(&img.data, &[255, 255, 255, 255]);
    }

    #[test]
    fn metadata_minimal_png() {
        let png_data = create_1x1_white_png();
        let decoder = StandardDecoder::new();
        let meta = decoder.metadata(&png_data).unwrap();
        assert_eq!(meta.width, 1);
        assert_eq!(meta.height, 1);
        assert!(meta.format.contains("Png"));
    }

    /// テスト用: 1x1 白ピクセルのPNGバイナリを生成
    fn create_1x1_white_png() -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }
}
