use crate::image::DecodedImage;
use crate::selection::PixelRect;

pub fn blur(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    box_blur(image, region, 1)
}

pub fn blur_strong(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    box_blur(image, region, 2)
}

pub fn median_filter(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    let w_u = image.width as usize;
    let w_i = image.width as i32;
    let h_i = image.height as i32;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = super::region_bounds(region, image.width, image.height);

    for y in y0..y1 {
        for x in x0..x1 {
            for ch in 0..3 {
                let mut values = Vec::with_capacity(9);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let nx = (x + dx).clamp(0, w_i - 1) as usize;
                        let ny = (y + dy).clamp(0, h_i - 1) as usize;
                        values.push(image.data[(ny * w_u + nx) * 4 + ch]);
                    }
                }
                values.sort_unstable();
                let offset = (y as usize * w_u + x as usize) * 4 + ch;
                data[offset] = values[4];
            }
        }
    }

    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

fn box_blur(image: &DecodedImage, region: Option<&PixelRect>, radius: i32) -> DecodedImage {
    let w_u = image.width as usize;
    let w_i = image.width as i32;
    let h_i = image.height as i32;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = super::region_bounds(region, image.width, image.height);
    let kernel_size = (2 * radius + 1) * (2 * radius + 1);

    for y in y0..y1 {
        for x in x0..x1 {
            for ch in 0..3 {
                let mut sum = 0u32;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = (x + dx).clamp(0, w_i - 1) as usize;
                        let ny = (y + dy).clamp(0, h_i - 1) as usize;
                        sum += image.data[(ny * w_u + nx) * 4 + ch] as u32;
                    }
                }
                let offset = (y as usize * w_u + x as usize) * 4 + ch;
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

pub fn mosaic(image: &DecodedImage, region: Option<&PixelRect>, block_size: u32) -> DecodedImage {
    let block = block_size.max(1) as i32;
    let w_u = image.width as usize;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = super::region_bounds(region, image.width, image.height);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let bx_end = (bx + block).min(x1);
            let by_end = (by + block).min(y1);
            let count = ((bx_end - bx) * (by_end - by)) as u32;

            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            for py in by..by_end {
                for px in bx..bx_end {
                    let offset = (py as usize * w_u + px as usize) * 4;
                    r_sum += image.data[offset] as u32;
                    g_sum += image.data[offset + 1] as u32;
                    b_sum += image.data[offset + 2] as u32;
                }
            }
            let avg_r = (r_sum / count) as u8;
            let avg_g = (g_sum / count) as u8;
            let avg_b = (b_sum / count) as u8;

            for py in by..by_end {
                for px in bx..bx_end {
                    let offset = (py as usize * w_u + px as usize) * 4;
                    data[offset] = avg_r;
                    data[offset + 1] = avg_g;
                    data[offset + 2] = avg_b;
                }
            }

            bx += block;
        }
        by += block;
    }

    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

pub fn gaussian_blur(
    image: &DecodedImage,
    region: Option<&PixelRect>,
    radius: f64,
) -> DecodedImage {
    let r = (radius.max(0.1) * 1.0).round() as i32;
    let r = r.max(1);
    let pass1 = box_blur(image, region, r);
    let pass2 = box_blur(&pass1, region, r);
    box_blur(&pass2, region, r)
}

pub fn unsharp_mask(image: &DecodedImage, region: Option<&PixelRect>, radius: f64) -> DecodedImage {
    let blurred = gaussian_blur(image, region, radius);
    let w_u = image.width as usize;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = super::region_bounds(region, image.width, image.height);

    for y in y0..y1 {
        for x in x0..x1 {
            let offset = (y as usize * w_u + x as usize) * 4;
            for ch in 0..3 {
                let orig = image.data[offset + ch] as i32;
                let blur_val = blurred.data[offset + ch] as i32;
                let v = orig + (orig - blur_val);
                data[offset + ch] = v.clamp(0, 255) as u8;
            }
        }
    }

    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::checker_image;
    use crate::test_helpers::uniform_image;

    #[test]
    fn blur_uniform_unchanged() {
        let img = uniform_image(4, 4, 100);
        let result = blur(&img, None);
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
        img.data[3] = 128;
        let result = blur(&img, None);
        assert_eq!(result.data[3], 128);
    }

    #[test]
    fn gaussian_blur_uniform_unchanged() {
        let img = uniform_image(8, 8, 120);
        let result = gaussian_blur(&img, None, 2.0);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 120);
        }
    }

    #[test]
    fn gaussian_blur_min_radius() {
        let img = uniform_image(4, 4, 80);
        let result = gaussian_blur(&img, None, 0.1);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 80);
        }
    }

    #[test]
    fn gaussian_blur_large_radius() {
        let img = checker_image(8, 8, 0, 200);
        let result = gaussian_blur(&img, None, 10.0);
        assert_eq!(result.width, 8);
        assert_eq!(result.height, 8);
        let center = ((4 * 8 + 4) * 4) as usize;
        let center_val = result.data[center] as i32;
        assert!(
            (center_val - 100).abs() < 30,
            "中央ピクセルが平均値 (100) に近いはず: got {center_val}"
        );
    }

    #[test]
    fn gaussian_blur_smooths_checker() {
        let img = checker_image(8, 8, 0, 200);
        let result = gaussian_blur(&img, None, 2.0);
        let mut min_val = 255u8;
        let mut max_val = 0u8;
        for pixel in result.data.chunks_exact(4) {
            min_val = min_val.min(pixel[0]);
            max_val = max_val.max(pixel[0]);
        }
        let range = max_val - min_val;
        assert!(
            range < 200,
            "ぼかし後のレンジが元の200未満のはず: got {range}"
        );
    }

    #[test]
    fn gaussian_blur_preserves_alpha() {
        let mut img = uniform_image(4, 4, 100);
        img.data[3] = 50;
        let result = gaussian_blur(&img, None, 2.0);
        assert_eq!(result.data[3], 50);
    }

    #[test]
    fn unsharp_mask_uniform_unchanged() {
        let img = uniform_image(8, 8, 150);
        let result = unsharp_mask(&img, None, 2.0);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 150);
        }
    }

    #[test]
    fn unsharp_mask_min_radius() {
        let img = uniform_image(4, 4, 100);
        let result = unsharp_mask(&img, None, 0.1);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 100);
        }
    }

    #[test]
    fn unsharp_mask_large_radius() {
        let img = checker_image(8, 8, 50, 200);
        let result = unsharp_mask(&img, None, 10.0);
        assert_eq!(result.width, 8);
        assert_eq!(result.height, 8);
        let mut min_val = 255u8;
        let mut max_val = 0u8;
        for pixel in result.data.chunks_exact(4) {
            min_val = min_val.min(pixel[0]);
            max_val = max_val.max(pixel[0]);
        }
        let original_range = 150;
        let result_range = max_val - min_val;
        assert!(
            result_range >= original_range as u8,
            "アンシャープマスクでコントラスト強調されるはず: original={original_range}, result={result_range}"
        );
    }

    #[test]
    fn unsharp_mask_preserves_alpha() {
        let mut img = uniform_image(4, 4, 100);
        img.data[7] = 42;
        let result = unsharp_mask(&img, None, 2.0);
        assert_eq!(result.data[7], 42);
    }

    #[test]
    fn unsharp_mask_clamps_values() {
        let img = checker_image(4, 4, 0, 255);
        let result = unsharp_mask(&img, None, 2.0);
        assert_eq!(result.data.len(), (4 * 4 * 4) as usize);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
    }

    #[test]
    fn mosaic_block_size_1_identity() {
        let img = checker_image(4, 4, 30, 220);
        let result = mosaic(&img, None, 1);
        assert_eq!(result.data, img.data);
    }

    #[test]
    fn mosaic_block_size_0_treated_as_1() {
        let img = checker_image(4, 4, 30, 220);
        let result = mosaic(&img, None, 0);
        assert_eq!(result.data, img.data);
    }

    #[test]
    fn mosaic_uniform_unchanged() {
        let img = uniform_image(8, 8, 100);
        let result = mosaic(&img, None, 4);
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], 100);
        }
    }

    #[test]
    fn mosaic_large_block_averages_all() {
        let img = checker_image(4, 4, 0, 200);
        let result = mosaic(&img, None, 100);
        let expected = 100u8;
        for pixel in result.data.chunks_exact(4) {
            assert_eq!(pixel[0], expected);
            assert_eq!(pixel[1], expected);
            assert_eq!(pixel[2], expected);
        }
    }

    #[test]
    fn mosaic_2x2_block_on_checker() {
        let img = checker_image(4, 4, 0, 200);
        let result = mosaic(&img, None, 2);
        for by in (0..4).step_by(2) {
            for bx in (0..4).step_by(2) {
                let offset0 = ((by * 4 + bx) * 4) as usize;
                let block_val = result.data[offset0];
                for dy in 0..2 {
                    for dx in 0..2 {
                        let offset = (((by + dy) * 4 + (bx + dx)) * 4) as usize;
                        assert_eq!(result.data[offset], block_val);
                        assert_eq!(result.data[offset + 1], block_val);
                        assert_eq!(result.data[offset + 2], block_val);
                    }
                }
            }
        }
    }

    #[test]
    fn mosaic_preserves_alpha() {
        let mut img = uniform_image(4, 4, 100);
        img.data[3] = 42;
        let result = mosaic(&img, None, 2);
        assert_eq!(result.data[3], 42);
    }
}
