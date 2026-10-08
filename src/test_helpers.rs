//! テスト共通の画像素材とオブジェクト生成。
//!
//! テストの一時フォルダはTempDirで作成し、失敗時の巻き戻しでも回収する。
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::archive::ArchiveManager;
use crate::document::{Document, DocumentEvent, ZipBuffer};
use crate::extension_registry::ExtensionRegistry;
use crate::file_info::{FileInfo, FileSource};
use crate::file_list::SortOrder;
use crate::image::{DecodedImage, DecoderChain, StandardDecoder};
use std::io::Write as _;

/// 一意な一時フォルダを所有し、破棄時に回収する。
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(stem: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        for sequence in 0u64.. {
            let path = std::env::temp_dir().join(format!(
                "gv_test_{stem}_{}_{nanos}_{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("テスト用フォルダを作成できません: {e}"),
            }
        }
        unreachable!("一時フォルダの連番が上限に達しました")
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.path()
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 単色の画像を作成する。
pub fn solid_image(width: u32, height: u32, rgba: [u8; 4]) -> DecodedImage {
    DecodedImage {
        data: rgba.repeat((width * height) as usize),
        width,
        height,
    }
}

/// テスト素材の画像をPNGとして保存する。
pub fn write_png(path: &Path, image: &DecodedImage) {
    image::RgbaImage::from_raw(image.width, image.height, image.data.clone())
        .expect("テスト素材の画素数が寸法と一致すること")
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

/// テスト用DecoderChainを生成する (StandardDecoderのみ)
pub fn test_decoder() -> Arc<DecoderChain> {
    Arc::new(DecoderChain::new(vec![Box::new(StandardDecoder::new())]))
}

/// テスト用ArchiveManagerを生成する
pub fn test_archive_manager(registry: &Arc<ExtensionRegistry>) -> ArchiveManager {
    ArchiveManager::new(Arc::clone(registry))
}

/// テスト用ZIPバッファを生成する
pub fn test_zip_buffers() -> Arc<RwLock<HashMap<PathBuf, ZipBuffer>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

#[test]
fn temp_dir_is_unique_and_removed_during_unwind() {
    let first = TempDir::new("guard");
    let second = TempDir::new("guard");
    assert_ne!(first.path(), second.path());
    let path = second.to_path_buf();
    let created_file = path.join("image.png");
    let result = std::panic::catch_unwind(move || {
        let _owned = second;
        std::fs::write(created_file, b"partial").unwrap();
        panic!("テスト失敗の巻き戻し");
    });
    assert!(result.is_err());
    assert!(first.exists());
    assert!(!path.exists());
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

/// tEXtチャンクを持つPNG素材。
pub fn create_1x1_png_with_text() -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut buf), 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // tEXtチャンクを追加
        let text_chunk =
            png::text_metadata::TEXtChunk::new("Author".to_string(), "TestAuthor".to_string());
        let _ = encoder.add_text_chunk(text_chunk.keyword, text_chunk.text);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 255, 255, 255]).unwrap();
    }
    buf
}

pub fn test_registry() -> Arc<ExtensionRegistry> {
    Arc::new(ExtensionRegistry::new())
}

/// テスト用のダミー画像ファイルを作成するヘルパー
pub fn create_test_files(dir: &Path, names: &[&str]) {
    let _ = std::fs::create_dir_all(dir);
    for name in names {
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(b"dummy").unwrap();
    }
}

/// テスト用のアーカイブエントリFileInfoを作成するヘルパー
pub fn make_archive_file_info(archive: &str, entry: &str, file_name: &str) -> FileInfo {
    FileInfo {
        file_name: file_name.to_string(),
        file_size: 100,
        modified: std::time::SystemTime::UNIX_EPOCH,
        marked: false,
        load_failed: false,
        source: FileSource::ArchiveEntry {
            archive: std::path::PathBuf::from(archive),
            entry: entry.to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        },
    }
}

/// テスト用の PendingContainer FileInfo を作成するヘルパー
pub fn make_pending_container_info(container_path: &str) -> FileInfo {
    FileInfo {
        file_name: container_path.to_string(),
        file_size: 0,
        modified: std::time::SystemTime::UNIX_EPOCH,
        marked: false,
        load_failed: false,
        source: FileSource::PendingContainer {
            container_path: std::path::PathBuf::from(container_path),
        },
    }
}

/// テスト用に任意の `FileSource` から `FileInfo` を生成するヘルパー。
/// 表示名はソースの論理パスから生成する。
pub fn make_file_info_with_source(
    source: FileSource,
    size: u64,
    modified: std::time::SystemTime,
) -> FileInfo {
    let path = match &source {
        FileSource::File(p) => p.clone(),
        FileSource::ArchiveEntry { archive, entry, .. } => archive.join(entry),
        FileSource::PdfPage {
            pdf_path,
            page_index,
        } => pdf_path.with_file_name(format!("page{page_index}.png")),
        FileSource::PendingContainer { container_path } => container_path.clone(),
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    FileInfo {
        file_name,
        file_size: size,
        modified,
        marked: false,
        load_failed: false,
        source,
    }
}

/// テスト用ZIPをメモリ上で作成してtempに保存する
pub fn create_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
}

/// ZIPをメモリ上で作成しバイト列として返す
pub fn create_test_zip_buffer(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

/// ファイル名フィールドにCP932バイト列を直接書き込んだ最小ZIPバイナリを組み立てる
///
/// 目的: zipクレートの`ZipWriter`はファイル名をUTF-8で書き込み汎用ビットフラグの
/// 第11ビット (UTF-8) を設定するため、後処理ではCP932記録ZIPを再現できない。
/// 本関数はZIP仕様 (APPNOTE 4.3) に従い、ローカルファイルヘッダー・セントラル
/// ディレクトリ・EOCDの長さとオフセットを整合させた上で、ファイル名フィールドへ
/// CP932バイト列をそのまま書き込む。
///
/// データはStored (無圧縮) で書き込み、CRC32は標準的なIEEEテーブルで計算する。
pub fn build_cp932_zip(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
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

        // Central directory record (構築は後段で行い、各エントリのバイト列だけを生成する)
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

pub fn decode_file(path: &Path) -> DecodedImage {
    let rgba = image::open(path).unwrap().into_rgba8();
    let (width, height) = rgba.dimensions();
    DecodedImage {
        data: rgba.into_raw(),
        width,
        height,
    }
}

pub fn assert_same_image(actual: &DecodedImage, expected: &DecodedImage) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert_eq!(actual.data, expected.data);
}

/// バッチのバイト列から、テストに不要な行を無効化する。
/// ASCIIプレフィクスで行を特定し、他の行の文字コードを保持する。
pub fn neutralize_batch_line(bytes: &[u8], ascii_prefix: &[u8]) -> Vec<u8> {
    let crlf = b"\r\n";
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        // 行末 (次のCRLFまたはEOF) を探す
        let line_end = bytes[pos..]
            .windows(2)
            .position(|w| w == crlf)
            .map_or(bytes.len(), |p| pos + p);
        let line = &bytes[pos..line_end];

        if line.starts_with(ascii_prefix) {
            // "rem " + 元の行でコメントアウト
            result.extend_from_slice(b"rem ");
            result.extend_from_slice(line);
        } else {
            result.extend_from_slice(line);
        }

        if line_end + 2 <= bytes.len() {
            result.extend_from_slice(crlf);
            pos = line_end + 2;
        } else {
            pos = bytes.len();
        }
    }
    result
}

/// テスト用の一時ディレクトリにダミー画像を配置する
pub fn setup_test_dir(name: &str, count: usize) -> TempDir {
    let dir = TempDir::new(name);
    std::fs::create_dir_all(&dir).unwrap();
    let png_data = create_1x1_white_png();
    for i in 0..count {
        let path = dir.join(format!("image_{i:03}.png"));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&png_data).unwrap();
    }
    dir
}

pub fn persistent_filter_image() -> DecodedImage {
    DecodedImage {
        data: vec![100, 150, 200, 255],
        width: 1,
        height: 1,
    }
}

pub fn color_filter_image() -> DecodedImage {
    DecodedImage {
        data: vec![
            100, 150, 200, 255, // pixel (0,0)
            50, 100, 150, 128, // pixel (1,0)
        ],
        width: 2,
        height: 1,
    }
}

/// 4x4のテスト画像を作成 (各ピクセルが座標で識別可能)
pub fn test_image_4x4() -> DecodedImage {
    let mut data = Vec::with_capacity(4 * 4 * 4);
    for y in 0..4u8 {
        for x in 0..4u8 {
            data.extend_from_slice(&[x * 60, y * 60, 0, 255]);
        }
    }
    DecodedImage {
        data,
        width: 4,
        height: 4,
    }
}

pub fn cache_image(size: usize) -> DecodedImage {
    DecodedImage {
        data: vec![0u8; size],
        width: 1,
        height: 1,
    }
}

/// テスト用のアーカイブ判定クロージャ
pub fn test_is_archive(p: &Path) -> bool {
    crate::extension_registry::ExtensionRegistry::new().is_archive_extension(&p.to_string_lossy())
}

/// テスト用に文字列を UTF-16 LE バイト列に変換する
pub fn to_utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

pub fn test_draw_rect() -> crate::render::layout::DrawRect {
    // 画像100x100を画面の (10,10)-(210,210) に描画している想定
    crate::render::layout::DrawRect {
        x: 10.0,
        y: 10.0,
        width: 200.0,
        height: 200.0,
    }
}

pub fn benchmark_trial(
    input: &str,
    scenario: &str,
    ms: f64,
    drawn: bool,
) -> crate::app::benchmark::Trial {
    crate::app::benchmark::Trial {
        input: input.to_string(),
        scenario: scenario.to_string(),
        round: 0,
        step: 0,
        index: 0,
        ms,
        drawn,
        cached: false,
    }
}

pub fn uniform_image(w: u32, h: u32, value: u8) -> DecodedImage {
    let data = vec![[value, value, value, 255u8]; (w * h) as usize]
        .into_iter()
        .flatten()
        .collect();
    DecodedImage {
        data,
        width: w,
        height: h,
    }
}

/// チェッカーパターン画像を生成 (偶数座標=c1, 奇数座標=c2)
pub fn checker_image(w: u32, h: u32, c1: u8, c2: u8) -> DecodedImage {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let offset = ((y * w + x) * 4) as usize;
            let v = if (x + y) % 2 == 0 { c1 } else { c2 };
            data[offset] = v;
            data[offset + 1] = v;
            data[offset + 2] = v;
            data[offset + 3] = 255;
        }
    }
    DecodedImage {
        data,
        width: w,
        height: h,
    }
}

/// 半透明の画素を含む8x6画像 (チェッカー背景と縮小ビットマップの作成に到達させる)
pub fn translucent_image() -> DecodedImage {
    DecodedImage {
        data: [200, 100, 50, 128].repeat(8 * 6),
        width: 8,
        height: 6,
    }
}

pub fn rect_tuple(rect: &crate::render::layout::DrawRect) -> (f32, f32, f32, f32) {
    (rect.x, rect.y, rect.width, rect.height)
}

/// 試験用の表示しないウィンドウ (クラス登録不要なSTATICを使う)。
pub struct HiddenWindow(pub windows::Win32::Foundation::HWND);

impl HiddenWindow {
    pub fn new() -> Self {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WINDOW_EX_STYLE, WS_OVERLAPPED,
        };
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::w!("STATIC"),
                None,
                WS_OVERLAPPED,
                0,
                0,
                320,
                240,
                None,
                None,
                None,
                None,
            )
        }
        .expect("テスト用ウィンドウの作成に失敗しました");
        Self(hwnd)
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0);
        }
    }
}

/// 96DPIで4×2画素、アプリの150DPI描画で6×3画素になる白い1ページのPDF。
pub fn minimal_pdf() -> Vec<u8> {
    let mut output = b"%PDF-1.4\n".to_vec();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 3 1.5] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ];
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(output.len());
        output.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = output.len();
    output.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    output.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    output
}

/// 実Shell操作のテストスレッドでSTAの初期化と解放を対応させる。
#[cfg(test)]
pub struct StaComGuard(std::marker::PhantomData<std::rc::Rc<()>>);

#[cfg(test)]
impl StaComGuard {
    pub fn init() -> anyhow::Result<Self> {
        use windows::Win32::Foundation::{S_FALSE, S_OK};
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

        // SAFETY: 予約引数はNone。現在のテストスレッドをSTAとして初期化する。
        // 成功時だけguardを生成し、!Sendかつ!Syncの所有者が同じスレッドで一度解放する。
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if status == S_OK || status == S_FALSE {
            Ok(Self(std::marker::PhantomData))
        } else {
            Err(windows::core::Error::from_hresult(status).into())
        }
    }
}

#[cfg(test)]
impl Drop for StaComGuard {
    fn drop(&mut self) {
        // SAFETY: initがS_OK/S_FALSEを返した場合だけ生成されるguardである。
        // RcのPhantomDataにより別スレッドへ移動できず、成功した初期化1回へ解放1回を対応させる。
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}
