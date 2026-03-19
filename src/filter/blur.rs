//! ぼかし・モザイクフィルタ

use crate::image::DecodedImage;
use crate::selection::PixelRect;

/// 3x3 ぼかし（平均フィルタ）
pub fn blur(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    box_blur(image, region, 1)
}

/// 5x5 ぼかし（強）
pub fn blur_strong(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    box_blur(image, region, 2)
}

/// メディアンフィルタ（3x3）
pub fn median_filter(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    let w = image.width as i32;
    let h = image.height as i32;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = region_bounds(region, image.width, image.height);

    for y in y0..y1 {
        for x in x0..x1 {
            for ch in 0..3 {
                let mut values = Vec::with_capacity(9);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let nx = (x + dx).clamp(0, w - 1);
                        let ny = (y + dy).clamp(0, h - 1);
                        values.push(image.data[((ny * w + nx) * 4 + ch) as usize]);
                    }
                }
                values.sort_unstable();
                let offset = ((y * w + x) * 4 + ch) as usize;
                data[offset] = values[4]; // 中央値
            }
        }
    }

    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

/// ボックスブラー（半径指定）
fn box_blur(image: &DecodedImage, region: Option<&PixelRect>, radius: i32) -> DecodedImage {
    let w = image.width as i32;
    let h = image.height as i32;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = region_bounds(region, image.width, image.height);
    let kernel_size = (2 * radius + 1) * (2 * radius + 1);

    for y in y0..y1 {
        for x in x0..x1 {
            for ch in 0..3 {
                let mut sum = 0u32;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = (x + dx).clamp(0, w - 1);
                        let ny = (y + dy).clamp(0, h - 1);
                        sum += image.data[((ny * w + nx) * 4 + ch) as usize] as u32;
                    }
                }
                let offset = ((y * w + x) * 4 + ch) as usize;
                data[offset] = (sum / kernel_size as u32) as u8;
            }
        }
    }

    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

/// 領域境界を計算
fn region_bounds(region: Option<&PixelRect>, width: u32, height: u32) -> (i32, i32, i32, i32) {
    if let Some(r) = region {
        let r = r.clamped(width, height);
        (r.x, r.y, r.right(), r.bottom())
    } else {
        (0, 0, width as i32, height as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform_image(w: u32, h: u32, value: u8) -> DecodedImage {
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

    #[test]
    fn blur_uniform_unchanged() {
        let img = uniform_image(4, 4, 100);
        let result = blur(&img, None);
        // 均一画像にぼかしをかけても値は変わらない
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 100);
        }
    }

    #[test]
    fn blur_strong_uniform_unchanged() {
        let img = uniform_image(6, 6, 50);
        let result = blur_strong(&img, None);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 50);
        }
    }

    #[test]
    fn median_uniform_unchanged() {
        let img = uniform_image(4, 4, 200);
        let result = median_filter(&img, None);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 200);
        }
    }

    #[test]
    fn blur_preserves_alpha() {
        let mut img = uniform_image(3, 3, 100);
        img.data[3] = 128; // 1ピクセルだけalpha変更
        let result = blur(&img, None);
        // alpha チャネルは変更されない
        assert_eq!(result.data[3], 128);
    }
}
