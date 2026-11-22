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

    fn gray_pair(left: u8, right: u8) -> DecodedImage {
        DecodedImage {
            width: 2,
            height: 1,
            data: vec![left, left, left, 255, right, right, right, 255],
        }
    }

    #[test]
    fn blur_uniform_unchanged() {
        let img = uniform_image(4, 4, 100);
        let result = blur(&img, None);
        assert_eq!((result.width, result.height), (4, 4));
        assert_eq!(result.data, img.data);
        let mut impulse = uniform_image(5, 5, 0);
        impulse.data[48] = 90;
        assert_eq!(blur(&impulse, None).data[48], 90 / 9);
    }

    #[test]
    fn blur_strong_uniform_unchanged() {
        let img = uniform_image(6, 6, 50);
        assert_eq!(blur_strong(&img, None).data, img.data);
        let mut impulse = uniform_image(5, 5, 0);
        impulse.data[48] = 225;
        assert_eq!(blur_strong(&impulse, None).data[48], 225 / 25);
    }

    #[test]
    fn median_uniform_unchanged() {
        let img = uniform_image(4, 4, 200);
        assert_eq!(median_filter(&img, None).data, img.data);
        let mut impulse = uniform_image(3, 3, 0);
        impulse.data[16] = 90;
        assert_eq!(median_filter(&impulse, None).data[16], 0);
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
        let img = crate::test_helpers::translucent_image();
        let result = gaussian_blur(&img, None, 2.0);
        assert_eq!((result.width, result.height), (8, 6));
        assert_eq!(result.data, img.data);
    }

    #[test]
    fn gaussian_blur_min_radius() {
        let img = gray_pair(60, 141);
        let result = gaussian_blur(&img, None, 0.1);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.data, gaussian_blur(&img, None, 1.0).data);
        assert_eq!(result.data, gray_pair(99, 102).data);
    }

    #[test]
    fn gaussian_blur_large_radius() {
        let img = gray_pair(40, 103);
        let result = gaussian_blur(&img, None, 10.0);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.data, gray_pair(71, 71).data);
    }

    #[test]
    fn gaussian_blur_smooths_checker() {
        let img = checker_image(8, 8, 0, 200);
        let result = gaussian_blur(&img, None, 1.0);
        assert_eq!(&result.data[108..116], &[98, 98, 98, 255, 99, 99, 99, 255]);
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
        let img = crate::test_helpers::translucent_image();
        let result = unsharp_mask(&img, None, 2.0);
        assert_eq!((result.width, result.height), (8, 6));
        assert_eq!(result.data, img.data);
    }

    #[test]
    fn unsharp_mask_min_radius() {
        let img = gray_pair(60, 141);
        let result = unsharp_mask(&img, None, 0.1);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.data, unsharp_mask(&img, None, 1.0).data);
        assert_eq!(result.data, gray_pair(21, 180).data);
    }

    #[test]
    fn unsharp_mask_large_radius() {
        let img = gray_pair(40, 103);
        let result = unsharp_mask(&img, None, 10.0);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.data, gray_pair(9, 135).data);
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
        let img = gray_pair(0, 255);
        let result = unsharp_mask(&img, None, 2.0);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.data, img.data);
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
        let img = crate::test_helpers::translucent_image();
        let result = mosaic(&img, None, 4);
        assert_eq!((result.width, result.height), (8, 6));
        assert_eq!(result.data, img.data);
        let mut strip = uniform_image(4, 1, 0);
        strip.data[0] = 30;
        strip.data[12] = 90;
        let result = mosaic(&strip, None, 3);
        assert_eq!(
            result.data,
            [10, 0, 0, 255, 10, 0, 0, 255, 10, 0, 0, 255, 90, 0, 0, 255]
        );
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
        let mut img = uniform_image(4, 2, 0);
        for (pixel, red) in img
            .data
            .chunks_exact_mut(4)
            .zip([0, 20, 100, 120, 40, 60, 140, 160])
        {
            pixel[0] = red;
        }
        let result = mosaic(&img, None, 2);
        assert_eq!(
            result.data,
            [
                [30, 0, 0, 255],
                [30, 0, 0, 255],
                [130, 0, 0, 255],
                [130, 0, 0, 255]
            ]
            .concat()
            .repeat(2)
        );
    }

    #[test]
    fn mosaic_preserves_alpha() {
        let mut img = uniform_image(4, 4, 100);
        img.data[3] = 42;
        let result = mosaic(&img, None, 2);
        assert_eq!(result.data[3], 42);
    }
}
