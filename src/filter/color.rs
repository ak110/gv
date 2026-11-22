use crate::image::DecodedImage;
use crate::selection::PixelRect;

pub fn invert_colors(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    super::apply_to_region(image, region, |r, g, b, a| [255 - r, 255 - g, 255 - b, a])
}

pub fn grayscale_simple(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    super::apply_to_region(image, region, |r, g, b, a| {
        let gray = ((r as u16 + g as u16 + b as u16) / 3) as u8;
        [gray, gray, gray, a]
    })
}

pub fn grayscale_strict(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    super::apply_to_region(image, region, |r, g, b, a| {
        let gray = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as u8;
        [gray, gray, gray, a]
    })
}

pub fn fill(image: &DecodedImage, region: Option<&PixelRect>, r: u8, g: u8, b: u8) -> DecodedImage {
    super::apply_to_region(image, region, |_r, _g, _b, a| [r, g, b, a])
}

pub fn apply_alpha(image: &DecodedImage, region: Option<&PixelRect>) -> DecodedImage {
    super::apply_to_region(image, region, |r, g, b, a| {
        let af = a as f32 / 255.0;
        let blend = |c: u8| (c as f32 * af + 255.0 * (1.0 - af)).round() as u8;
        [blend(r), blend(g), blend(b), 255]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::color_filter_image as test_image;

    #[test]
    fn invert_colors_basic() {
        let img = test_image();
        let result = invert_colors(&img, None);
        assert_eq!(result.data[0], 155);
        assert_eq!(result.data[1], 105);
        assert_eq!(result.data[2], 55);
        assert_eq!(result.data[3], 255);
    }

    #[test]
    fn grayscale_simple_average() {
        let img = DecodedImage {
            data: vec![30, 60, 90, 255],
            width: 1,
            height: 1,
        };
        let result = grayscale_simple(&img, None);
        assert_eq!(result.data[0], 60);
        assert_eq!(result.data[1], 60);
        assert_eq!(result.data[2], 60);
    }

    #[test]
    fn grayscale_strict_bt709() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64],
            width: 3,
            height: 1,
        };
        let result = grayscale_strict(&img, None);
        assert_eq!(
            result.data,
            [54, 54, 54, 255, 182, 182, 182, 128, 18, 18, 18, 64]
        );
    }

    #[test]
    fn apply_alpha_white_bg() {
        let img = DecodedImage {
            data: vec![0, 128, 255, 128],
            width: 1,
            height: 1,
        };
        let result = apply_alpha(&img, None);
        assert_eq!(result.data, [127, 191, 255, 255]);
    }

    #[test]
    fn region_only_modifies_selection() {
        let img = test_image();
        let region = PixelRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        let result = invert_colors(&img, Some(&region));
        assert_eq!(result.data, [155, 105, 55, 255, 50, 100, 150, 128]);
    }
}
