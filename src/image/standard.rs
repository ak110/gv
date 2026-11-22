use std::io::Cursor;

use anyhow::Context as _;

use super::{DecodedImage, ImageDecoder, ImageMetadata};

pub struct StandardDecoder;

const STANDARD_FORMATS: &[(&str, &[&str])] = &[
    ("JPEG", &[".jpg", ".jpeg"]),
    ("PNG", &[".png"]),
    ("GIF", &[".gif"]),
    ("BMP", &[".bmp"]),
    ("WebP", &[".webp"]),
];

impl StandardDecoder {
    pub fn new() -> Self {
        Self
    }

    pub fn formats() -> &'static [(&'static str, &'static [&'static str])] {
        STANDARD_FORMATS
    }

    pub fn extensions() -> impl Iterator<Item = &'static str> {
        STANDARD_FORMATS
            .iter()
            .flat_map(|(_, extensions)| extensions.iter().copied())
    }
}

impl ImageDecoder for StandardDecoder {
    fn can_decode(&self, data: &[u8], _filename_hint: &str) -> bool {
        image::guess_format(data).is_ok()
    }

    fn decode(&self, data: &[u8], _filename_hint: &str) -> anyhow::Result<DecodedImage> {
        let mut decoder = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .context("画像形式の判定に失敗")?
            .into_decoder()
            .context("画像のデコードに失敗")?;
        let orientation = image::ImageDecoder::orientation(&mut decoder)
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = image::DynamicImage::from_decoder(decoder).context("画像のデコードに失敗")?;
        img.apply_orientation(orientation);
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();

        Ok(DecodedImage {
            data: rgba.into_raw(),
            width,
            height,
        })
    }

    fn metadata(&self, data: &[u8], _filename_hint: &str) -> anyhow::Result<ImageMetadata> {
        let format =
            image::guess_format(data).map_or_else(|_| "Unknown".to_string(), |f| format!("{f:?}"));

        let comments = if matches!(image::guess_format(data), Ok(image::ImageFormat::Png)) {
            Self::read_png_text_chunks(data)
        } else {
            Vec::new()
        };

        let exif = super::read_exif_fields(data);

        Ok(ImageMetadata {
            format,
            comments,
            exif,
        })
    }
}

impl StandardDecoder {
    fn read_png_text_chunks(data: &[u8]) -> Vec<String> {
        let decoder = png::Decoder::new(Cursor::new(data));
        let Ok(reader) = decoder.read_info() else {
            return Vec::new();
        };
        let info = reader.info();
        let mut texts = Vec::new();

        for chunk in &info.uncompressed_latin1_text {
            texts.push(format!("{}: {}", chunk.keyword, chunk.text));
        }
        for chunk in &info.compressed_latin1_text {
            if let Ok(text) = chunk.get_text() {
                texts.push(format!("{}: {}", chunk.keyword, text));
            }
        }
        for chunk in &info.utf8_text {
            if let Ok(text) = chunk.get_text() {
                texts.push(format!("{}: {}", chunk.keyword, text));
            }
        }

        texts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{create_1x1_png_with_text, create_1x1_white_png};

    #[test]
    fn all_standard_extensions_decode_an_image() {
        let decoder = StandardDecoder::new();
        for extension in StandardDecoder::extensions() {
            let format = image::ImageFormat::from_extension(extension.trim_start_matches('.'))
                .expect("標準拡張子には画像形式がある");
            let sample = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                2,
                1,
                image::Rgb([20, 40, 60]),
            ));
            let mut encoded = Cursor::new(Vec::new());
            sample.write_to(&mut encoded, format).unwrap();
            let decoded = decoder
                .decode(encoded.get_ref(), &format!("sample{extension}"))
                .unwrap();
            assert_eq!((decoded.width, decoded.height), (2, 1), "{extension}");
        }
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let decoder = StandardDecoder::new();
        let result = decoder.decode(&[0, 1, 2, 3], "test.jpg");
        assert!(result.is_err());
    }

    #[test]
    fn can_decode_rejects_invalid_data() {
        let decoder = StandardDecoder::new();
        assert!(!decoder.can_decode(&[0, 1, 2, 3], "test.jpg"));
    }

    #[test]
    fn decode_minimal_png() {
        let png_data = create_1x1_white_png();
        let decoder = StandardDecoder::new();
        assert!(decoder.can_decode(&png_data, "test.png"));

        let img = decoder.decode(&png_data, "test.png").unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        assert_eq!(img.data.len(), 4);
        assert_eq!(&img.data, &[255, 255, 255, 255]);
    }

    #[test]
    fn metadata_minimal_png() {
        let png_data = create_1x1_white_png();
        let decoder = StandardDecoder::new();
        let meta = decoder.metadata(&png_data, "test.png").unwrap();
        assert!(meta.format.contains("Png"));
    }

    #[test]
    fn metadata_png_with_text_chunks() {
        let png_data = create_1x1_png_with_text();
        let decoder = StandardDecoder::new();
        let meta = decoder.metadata(&png_data, "test.png").unwrap();
        assert!(
            meta.comments.iter().any(|c| c.contains("Author")),
            "tEXtチャンクが取得できること: {:?}",
            meta.comments
        );
        assert!(
            meta.comments.iter().any(|c| c.contains("TestAuthor")),
            "tEXtチャンクの値が正しいこと: {:?}",
            meta.comments
        );
    }

    #[test]
    fn applies_all_exif_orientations() {
        use crate::test_helpers::{
            asymmetric_rgba, encode_with_exif, exif_with_orientation, expected_oriented,
        };
        let src = asymmetric_rgba(3, 2);
        let decoder = StandardDecoder::new();
        for orientation in 1..=8 {
            let data = encode_with_exif(
                &src,
                image::ImageFormat::Png,
                Some(exif_with_orientation(orientation)),
            );
            let img = decoder.decode(&data, "oriented.png").unwrap();
            let expected = expected_oriented(&src, orientation);
            assert_eq!(
                (img.width, img.height),
                expected.dimensions(),
                "orientation {orientation}"
            );
            assert_eq!(img.data, expected.into_raw(), "orientation {orientation}");
        }
    }

    #[test]
    fn applies_orientation_to_jpeg() {
        use crate::test_helpers::{encode_with_exif, exif_with_orientation};
        let src = image::RgbaImage::from_fn(32, 16, |x, _| {
            let v = if x < 16 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        });
        let data = encode_with_exif(
            &src,
            image::ImageFormat::Jpeg,
            Some(exif_with_orientation(6)),
        );
        let img = StandardDecoder::new()
            .decode(&data, "oriented.jpg")
            .unwrap();
        assert_eq!((img.width, img.height), (16, 32));
        let luma = |x: u32, y: u32| img.data[((y * img.width + x) * 4) as usize];
        assert!(luma(8, 8) < 40, "top should be black: {}", luma(8, 8));
        assert!(luma(8, 24) > 215, "bottom should be white: {}", luma(8, 24));
    }

    #[test]
    fn invalid_or_missing_orientation_keeps_pixels() {
        use crate::test_helpers::{asymmetric_rgba, encode_with_exif, exif_with_orientation};
        let src = asymmetric_rgba(3, 2);
        let decoder = StandardDecoder::new();
        for exif in [
            None,
            Some(exif_with_orientation(0)),
            Some(exif_with_orientation(9)),
            Some(b"II*\0garbage".to_vec()),
        ] {
            let label = format!("{exif:?}");
            let data = encode_with_exif(&src, image::ImageFormat::Png, exif);
            let img = decoder.decode(&data, "plain.png").unwrap();
            assert_eq!((img.width, img.height), (3, 2), "{label}");
            assert_eq!(img.data, src.as_raw().clone(), "{label}");
        }
    }
}
