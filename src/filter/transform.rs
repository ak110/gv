use std::borrow::Cow;

use anyhow::{Context as _, Result};

use crate::image::DecodedImage;
use crate::selection::PixelRect;

pub fn crop(image: &DecodedImage, rect: &PixelRect) -> DecodedImage {
    let rect = rect.clamped(image.width, image.height);
    if !rect.is_valid() {
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

pub fn output_image(image: &DecodedImage, selection: Option<PixelRect>) -> Cow<'_, DecodedImage> {
    match selection {
        Some(rect) => Cow::Owned(crop(image, &rect)),
        None => Cow::Borrowed(image),
    }
}

pub fn flip_horizontal(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst = (y * w + (w - 1 - x)) * 4;
            data[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
        }
    }
    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

pub fn flip_vertical(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let row_bytes = w * 4;
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_start = y * row_bytes;
        let dst_start = (h - 1 - y) * row_bytes;
        data[dst_start..dst_start + row_bytes]
            .copy_from_slice(&image.data[src_start..src_start + row_bytes]);
    }
    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

pub fn rotate_90(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst = (x * h + (h - 1 - y)) * 4;
            data[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
        }
    }
    DecodedImage {
        data,
        width: image.height,
        height: image.width,
    }
}

pub fn rotate_180(image: &DecodedImage) -> DecodedImage {
    let total = image.data.len();
    let mut data = vec![0u8; total];
    let pixel_count = total / 4;
    for i in 0..pixel_count {
        let src = i * 4;
        let dst = (pixel_count - 1 - i) * 4;
        data[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
    }
    DecodedImage {
        data,
        width: image.width,
        height: image.height,
    }
}

pub fn rotate_270(image: &DecodedImage) -> DecodedImage {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 4;
            let dst = ((w - 1 - x) * h + y) * 4;
            data[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
        }
    }
    DecodedImage {
        data,
        width: image.height,
        height: image.width,
    }
}

pub fn rotate_arbitrary(image: &DecodedImage, degrees: f64) -> DecodedImage {
    let rad = degrees.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let w = image.width as f64;
    let h = image.height as f64;

    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for (cx, cy) in &corners {
        let rx = cx * cos - cy * sin;
        let ry = cx * sin + cy * cos;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }

    let new_w = (max_x - min_x).ceil() as u32;
    let new_h = (max_y - min_y).ceil() as u32;
    if new_w == 0 || new_h == 0 {
        return DecodedImage {
            data: image.data.clone(),
            width: image.width,
            height: image.height,
        };
    }

    let src_w = image.width as usize;
    let mut data = vec![0u8; (new_w * new_h * 4) as usize];

    let cos_inv = cos;
    let sin_inv = -sin;
    for dy in 0..new_h {
        for dx in 0..new_w {
            let rx = dx as f64 + min_x;
            let ry = dy as f64 + min_y;
            let sx = rx * cos_inv - ry * sin_inv;
            let sy = rx * sin_inv + ry * cos_inv;

            let sx_floor = sx.floor();
            let sy_floor = sy.floor();
            let fx = sx - sx_floor;
            let fy = sy - sy_floor;
            let ix = sx_floor as i32;
            let iy = sy_floor as i32;

            if ix < 0 || iy < 0 || ix + 1 >= image.width as i32 || iy + 1 >= image.height as i32 {
                continue;
            }

            let dst = ((dy * new_w + dx) * 4) as usize;
            for ch in 0..4 {
                let get = |x: i32, y: i32| -> f64 {
                    image.data[(y as usize * src_w + x as usize) * 4 + ch] as f64
                };
                let v00 = get(ix, iy);
                let v10 = get(ix + 1, iy);
                let v01 = get(ix, iy + 1);
                let v11 = get(ix + 1, iy + 1);
                let v = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
                data[dst + ch] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    DecodedImage {
        data,
        width: new_w,
        height: new_h,
    }
}

pub fn resize(image: &DecodedImage, new_width: u32, new_height: u32) -> Result<DecodedImage> {
    if new_width == 0 || new_height == 0 {
        return Ok(DecodedImage {
            data: image.data.clone(),
            width: image.width,
            height: image.height,
        });
    }
    use fast_image_resize as fr;
    let src_image =
        fr::images::ImageRef::new(image.width, image.height, &image.data, fr::PixelType::U8x4)
            .context("リサイズ用ソース画像の作成に失敗")?;
    let mut dst_image = fr::images::Image::new(new_width, new_height, fr::PixelType::U8x4);
    let options =
        fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3));
    let mut resizer = fr::Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, &options)
        .context("画像リサイズに失敗")?;
    Ok(DecodedImage {
        data: dst_image.into_vec(),
        width: new_width,
        height: new_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_image_4x4;

    #[test]
    fn output_image_returns_whole_image_without_selection() {
        let img = test_image_4x4();
        let output = output_image(&img, None);
        assert_eq!(output.width, img.width);
        assert_eq!(output.height, img.height);
        assert_eq!(output.data, img.data);
    }

    #[test]
    fn output_image_extracts_selected_region() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        let cropped = output_image(&img, Some(rect));
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(
            cropped.data,
            [
                60, 60, 0, 255, 120, 60, 0, 255, 60, 120, 0, 255, 120, 120, 0, 255
            ]
        );
    }

    #[test]
    fn output_image_clamps_partially_outside_region() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 2,
            y: 2,
            width: 10,
            height: 10,
        };
        let cropped = output_image(&img, Some(rect));
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(
            cropped.data,
            [
                120, 120, 0, 255, 180, 120, 0, 255, 120, 180, 0, 255, 180, 180, 0, 255
            ]
        );
    }

    #[test]
    fn output_image_falls_back_to_whole_image_when_disjoint() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 100,
            y: 100,
            width: 20,
            height: 20,
        };
        let cropped = output_image(&img, Some(rect));
        assert_eq!(cropped.width, img.width);
        assert_eq!(cropped.height, img.height);
        assert_eq!(cropped.data, img.data);
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
        assert_eq!(
            cropped.data,
            [
                120, 120, 0, 255, 180, 120, 0, 255, 120, 180, 0, 255, 180, 180, 0, 255
            ]
        );
    }

    #[test]
    fn crop_zero_rect_returns_original() {
        let img = test_image_4x4();
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 0,
            height: 2,
        };
        let cropped = crop(&img, &rect);
        assert_eq!(cropped.width, img.width);
        assert_eq!(cropped.height, img.height);
        assert_eq!(cropped.data, img.data);
    }

    #[test]
    fn flip_horizontal_2x1() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 255, 0, 255, 0, 255],
            width: 2,
            height: 1,
        };
        let flipped = flip_horizontal(&img);
        assert_eq!(flipped.data[0..4], [0, 255, 0, 255]);
        assert_eq!(flipped.data[4..8], [255, 0, 0, 255]);
    }

    #[test]
    fn flip_vertical_1x2() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 255, 0, 255, 0, 255],
            width: 1,
            height: 2,
        };
        let flipped = flip_vertical(&img);
        assert_eq!(flipped.data[0..4], [0, 255, 0, 255]);
        assert_eq!(flipped.data[4..8], [255, 0, 0, 255]);
    }

    #[test]
    fn rotate_90_2x1() {
        let img = DecodedImage {
            data: [1, 2, 3, 4, 5, 6]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 3,
        };
        let rotated = rotate_90(&img);
        assert_eq!((rotated.width, rotated.height), (3, 2));
        let expected: Vec<u8> = [5, 3, 1, 6, 4, 2]
            .into_iter()
            .flat_map(|r| [r, 0, 0, 255])
            .collect();
        assert_eq!(rotated.data, expected);
    }

    #[test]
    fn rotate_180_identity() {
        let img = DecodedImage {
            data: [1, 2, 3, 4, 5, 6]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 3,
        };
        let rotated = rotate_180(&img);
        assert_eq!((rotated.width, rotated.height), (2, 3));
        let expected: Vec<u8> = [6, 5, 4, 3, 2, 1]
            .into_iter()
            .flat_map(|r| [r, 0, 0, 255])
            .collect();
        assert_eq!(rotated.data, expected);
    }

    #[test]
    fn rotate_270_is_3x_rotate_90() {
        let img = DecodedImage {
            data: [1, 2, 3, 4, 5, 6]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 3,
        };
        let rotated = rotate_270(&img);
        assert_eq!((rotated.width, rotated.height), (3, 2));
        let expected: Vec<u8> = [2, 4, 6, 1, 3, 5]
            .into_iter()
            .flat_map(|r| [r, 0, 0, 255])
            .collect();
        assert_eq!(rotated.data, expected);
    }

    #[test]
    fn resize_basic() {
        let img = DecodedImage {
            data: [30, 60, 90, 128].repeat(16),
            width: 4,
            height: 4,
        };
        let resized = resize(&img, 2, 2).expect("resize");
        assert_eq!(resized.width, 2);
        assert_eq!(resized.height, 2);
        assert_eq!(resized.data, [30, 60, 90, 128].repeat(4));
    }

    #[test]
    fn resize_zero_returns_original_clone() {
        let img = test_image_4x4();
        for (width, height) in [(0, 2), (2, 0)] {
            let r = resize(&img, width, height).expect("resize zero");
            assert_eq!((r.width, r.height), (img.width, img.height));
            assert_eq!(r.data, img.data);
        }
    }

    #[test]
    fn rotate_arbitrary_0_identity() {
        let img = test_image_4x4();
        let rotated = rotate_arbitrary(&img, 0.0);
        assert_eq!(rotated.width, img.width);
        assert_eq!(rotated.height, img.height);
        assert_eq!(rotated.data, img.data);
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
        assert_eq!(cropped.data, [180, 180, 0, 255]);
    }
}
