use crate::image::DecodedImage;
use crate::selection::PixelRect;

pub fn sharpen(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    apply_kernel(image, region, &SHARPEN_KERNEL)
}

pub fn sharpen_strong(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    apply_kernel(image, region, &SHARPEN_STRONG_KERNEL)
}

const SHARPEN_KERNEL: [i32; 9] = [0, -1, 0, -1, 5, -1, 0, -1, 0];
const SHARPEN_STRONG_KERNEL: [i32; 9] = [-1, -1, -1, -1, 9, -1, -1, -1, -1];

fn apply_kernel(
    image: &DecodedImage,
    region: Option<&PixelRect>,
    kernel: &[i32; 9],
) -> DecodedImage {
    let w = image.width as i32;
    let h = image.height as i32;
    let mut data = image.data.clone();

    let (x0, y0, x1, y1) = super::region_bounds(region, image.width, image.height);

    for y in y0..y1 {
        for x in x0..x1 {
            for ch in 0..3 {
                let mut sum = 0i32;
                let mut ki = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let nx = (x + dx).clamp(0, w - 1);
                        let ny = (y + dy).clamp(0, h - 1);
                        sum += image.data[((ny * w + nx) * 4 + ch) as usize] as i32 * kernel[ki];
                        ki += 1;
                    }
                }
                let offset = ((y * w + x) * 4 + ch) as usize;
                data[offset] = sum.clamp(0, 255) as u8;
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
    use crate::test_helpers::uniform_image;

    #[test]
    fn sharpen_uniform_unchanged() {
        let mut img = uniform_image(3, 3, 100);
        assert_eq!(sharpen(&img, None).data, img.data);

        img.data[16] = 110;
        assert_eq!(sharpen(&img, None).data[16], 150);
    }

    #[test]
    fn sharpen_strong_uniform_unchanged() {
        let mut img = uniform_image(3, 3, 100);
        assert_eq!(sharpen_strong(&img, None).data, img.data);

        img.data[0] = 110;
        assert_eq!(sharpen_strong(&img, None).data[16], 90);
    }

    #[test]
    fn sharpen_preserves_alpha() {
        let mut img = uniform_image(3, 3, 100);
        for (pixel, alpha) in img
            .data
            .chunks_exact_mut(4)
            .zip([1, 2, 3, 4, 15, 6, 7, 8, 9])
        {
            pixel[3] = alpha;
        }
        img.data[16] = 110;
        let result = sharpen(&img, None);
        assert_eq!(result.data[16], 150);
        assert_eq!(
            result
                .data
                .chunks_exact(4)
                .map(|p| p[3])
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 15, 6, 7, 8, 9]
        );
    }
}
