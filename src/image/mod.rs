mod exif_reader;
mod standard;
pub mod susie;

pub use exif_reader::read_exif_fields;
pub use standard::StandardDecoder;

#[derive(Clone)]
pub struct DecodedImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl DecodedImage {
    pub fn memory_size(&self) -> usize {
        self.data.len()
    }
}

pub struct ImageMetadata {
    pub format: String,
    pub comments: Vec<String>,
    pub exif: Vec<(String, String)>,
}

pub trait ImageDecoder: Send + Sync {
    fn can_decode(&self, data: &[u8], filename_hint: &str) -> bool;

    fn decode(&self, data: &[u8], filename_hint: &str) -> anyhow::Result<DecodedImage>;

    fn metadata(&self, data: &[u8], filename_hint: &str) -> anyhow::Result<ImageMetadata>;
}

pub struct DecoderChain {
    decoders: Vec<Box<dyn ImageDecoder>>,
}

impl DecoderChain {
    pub fn new(decoders: Vec<Box<dyn ImageDecoder>>) -> Self {
        Self { decoders }
    }

    pub fn metadata(&self, data: &[u8], filename_hint: &str) -> anyhow::Result<ImageMetadata> {
        let mut last_error = None;
        for decoder in &self.decoders {
            if decoder.can_decode(data, filename_hint) {
                match decoder.metadata(data, filename_hint) {
                    Ok(meta) => return Ok(meta),
                    Err(e) => last_error = Some(e),
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("対応するデコーダが存在しない: {filename_hint}")))
    }

    pub fn decode(&self, data: &[u8], filename_hint: &str) -> anyhow::Result<DecodedImage> {
        let mut last_error = None;
        for decoder in &self.decoders {
            if decoder.can_decode(data, filename_hint) {
                match decoder.decode(data, filename_hint) {
                    Ok(image) => return Ok(image),
                    Err(e) => last_error = Some(e),
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("対応するデコーダが存在しない: {filename_hint}")))
    }
}

pub fn decode_source(
    source: &crate::file_info::FileSource,
    decoder: &DecoderChain,
    manager: &crate::archive::ArchiveManager,
    buffers: &std::sync::RwLock<
        std::collections::HashMap<std::path::PathBuf, crate::document::ZipBuffer>,
    >,
    synchronous: bool,
) -> anyhow::Result<DecodedImage> {
    if let crate::file_info::FileSource::PdfPage {
        pdf_path,
        page_index,
    } = source
    {
        if synchronous {
            crate::pdf_renderer::render_pdf_page_safe(pdf_path, *page_index)
        } else {
            crate::pdf_renderer::render_pdf_page(pdf_path, *page_index)
        }
    } else {
        decoder.decode(
            &source.read_bytes(manager, buffers)?,
            &source.filename_hint(),
        )
    }
}

pub fn encode_png(image: &DecodedImage) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context as _;
    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.data.clone())
        .context("画像バッファの作成に失敗")?;
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buffer).write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}
