/// テスト共通ヘルパー
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::archive::ArchiveManager;
use crate::document::{Document, DocumentEvent, ZipBuffer};
use crate::extension_registry::ExtensionRegistry;
use crate::file_list::SortOrder;
use crate::image::{DecoderChain, StandardDecoder};

/// テスト用DecoderChainを生成する (StandardDecoderのみ)
pub fn test_decoder() -> Arc<DecoderChain> {
    Arc::new(DecoderChain::new(vec![Box::new(StandardDecoder::new())]))
}

/// テスト用ArchiveManagerを生成する
pub fn test_archive_manager(registry: &Arc<ExtensionRegistry>) -> ArchiveManager {
    ArchiveManager::new(Arc::clone(registry))
}

/// テスト用ZIPバッファを生成する
#[allow(dead_code)]
pub fn test_zip_buffers() -> Arc<RwLock<HashMap<PathBuf, ZipBuffer>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

/// テスト用Documentとイベントレシーバーを生成する
pub fn test_document() -> (Document, crossbeam_channel::Receiver<DocumentEvent>) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    let registry = Arc::new(ExtensionRegistry::new());
    let decoder = test_decoder();
    let archive_manager = test_archive_manager(&registry);
    let doc = Document::new(
        sender,
        decoder,
        registry,
        archive_manager,
        SortOrder::default(),
    );
    (doc, receiver)
}

/// テスト用: 1x1 白ピクセルのPNGバイナリを生成する
pub fn create_1x1_white_png() -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// テスト用: Orientationタグだけを持つEXIF (TIFF形式、リトルエンディアン) を生成する
pub fn exif_with_orientation(orientation: u16) -> Vec<u8> {
    let mut exif = Vec::new();
    exif.extend_from_slice(b"II*\0");
    exif.extend_from_slice(&8u32.to_le_bytes()); // 最初のIFDの位置
    exif.extend_from_slice(&1u16.to_le_bytes()); // エントリ数
    exif.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    exif.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    exif.extend_from_slice(&1u32.to_le_bytes()); // 個数
    exif.extend_from_slice(&orientation.to_le_bytes());
    exif.extend_from_slice(&[0, 0]); // 値欄の残り
    exif.extend_from_slice(&0u32.to_le_bytes()); // 次のIFDなし
    exif
}

/// テスト用: 画素ごとに異なる色を持つ非対称な画像 (向きの判定用)
pub fn asymmetric_rgba(width: u32, height: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x * 60) as u8, (y * 90) as u8, (x + y * width) as u8, 255])
    })
}

/// テスト用: EXIFを埋め込んだ画像をエンコードする (`exif`がNoneなら埋め込まない)
pub fn encode_with_exif(
    img: &image::RgbaImage,
    format: image::ImageFormat,
    exif: Option<Vec<u8>>,
) -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut buf = Vec::new();
    match format {
        image::ImageFormat::Png => {
            let mut encoder = image::codecs::png::PngEncoder::new(&mut buf);
            if let Some(exif) = exif {
                encoder.set_exif_metadata(exif).unwrap();
            }
            encoder
                .write_image(
                    img.as_raw(),
                    img.width(),
                    img.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
        }
        image::ImageFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 95);
            if let Some(exif) = exif {
                encoder.set_exif_metadata(exif).unwrap();
            }
            encoder
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .unwrap();
        }
        other => panic!("unsupported test format: {other:?}"),
    }
    buf
}

/// テスト用: EXIFの向き (1〜8) を仕様どおりに適用した期待画像を、画素の座標対応から求める
pub fn expected_oriented(src: &image::RgbaImage, orientation: u16) -> image::RgbaImage {
    let (w, h) = src.dimensions();
    let swapped = orientation >= 5;
    let (ow, oh) = if swapped { (h, w) } else { (w, h) };
    let mut out = image::RgbaImage::new(ow, oh);
    for (x, y, px) in src.enumerate_pixels() {
        let (dx, dy) = match orientation {
            2 => (w - 1 - x, y),
            3 => (w - 1 - x, h - 1 - y),
            4 => (x, h - 1 - y),
            5 => (y, x),
            6 => (h - 1 - y, x),
            7 => (h - 1 - y, w - 1 - x),
            8 => (y, w - 1 - x),
            _ => (x, y),
        };
        out.put_pixel(dx, dy, *px);
    }
    out
}
