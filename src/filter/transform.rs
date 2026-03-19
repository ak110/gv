//! 画像の幾何変換（トリミング等）

use crate::image::DecodedImage;
use crate::selection::PixelRect;

/// 画像をトリミングする
pub fn crop(image: &DecodedImage, rect: &PixelRect) -> DecodedImage {
    let rect = rect.clamped(image.width, image.height);
    if !rect.is_valid() {
        // 空の矩形の場合は元画像をそのまま返す
        return DecodedImage {
            data: image.data.clone(),
            width: image.width,
            height: image.height,
        };
    }

    let src_w = image.width as usize;
    let dst_w = rect.width as usize;
    let dst_h = rect.height as usize;
    let mut data = Vec::with_capacity(dst_w * dst_h * 4);

    for row in 0..dst_h {
        let src_y = rect.y as usize + row;
        let src_offset = (src_y * src_w + rect.x as usize) * 4;
        let src_end = src_offset + dst_w * 4;
        data.extend_from_slice(&image.data[src_offset..src_end]);
    }

    DecodedImage {
        data,
        width: rect.width as u32,
        height: rect.height as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4x4のテスト画像を作成（各ピクセルが座標で識別可能）
    fn test_image_4x4() -> DecodedImage {
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

    #[test]
    fn crop_center_2x2() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.data.len(), 2 * 2 * 4);
        // (1,1)のピクセル: R=60, G=60
        assert_eq!(cropped.data[0], 60);
        assert_eq!(cropped.data[1], 60);
    }

    #[test]
    fn crop_full_image() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, 4);
        assert_eq!(cropped.height, 4);
        assert_eq!(cropped.data, img.data);
    }

    #[test]
    fn crop_clamps_to_image_bounds() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 2,
            y: 2,
            width: 100,
            height: 100,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
    }

    #[test]
    fn crop_zero_rect_returns_original() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, img.width);
        assert_eq!(cropped.height, img.height);
    }

    #[test]
    fn crop_single_pixel() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 3,
            y: 3,
            width: 1,
            height: 1,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, 1);
        assert_eq!(cropped.height, 1);
        assert_eq!(cropped.data.len(), 4);
        // (3,3)のピクセル: R=3*60=180, G=3*60=180
        assert_eq!(cropped.data[0], 180);
        assert_eq!(cropped.data[1], 180);
    }
}
