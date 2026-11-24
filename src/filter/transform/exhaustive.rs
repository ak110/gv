use super::*;

fn image(width: u32, height: u32) -> DecodedImage {
    let data = (0..width * height)
        .flat_map(|i| [i as u8 + 1, 3 * i as u8, 200 - i as u8, 255])
        .collect();
    DecodedImage {
        data,
        width,
        height,
    }
}

fn pixel(img: &DecodedImage, x: u32, y: u32) -> &[u8] {
    let start = ((y * img.width + x) * 4) as usize;
    &img.data[start..start + 4]
}

fn assert_image(actual: &DecodedImage, expected: &DecodedImage) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert_eq!(actual.data, expected.data);
}

fn assert_mapping(
    actual: &DecodedImage,
    src: &DecodedImage,
    width: u32,
    height: u32,
    source_at: impl Fn(u32, u32) -> (u32, u32),
) {
    assert_eq!((actual.width, actual.height), (width, height));
    assert_eq!(actual.data.len(), (width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (sx, sy) = source_at(x, y);
            assert_eq!(pixel(actual, x, y), pixel(src, sx, sy), "({x}, {y})");
        }
    }
}

#[test]
fn crop_all_small_rectangles() {
    for w in 1..=4 {
        for h in 1..=4 {
            let src = image(w, h);
            assert_image(&output_image(&src, None), &src);
            for x in -1..=5 {
                for y in -1..=5 {
                    for width in -1..=5 {
                        for height in -1..=5 {
                            let r = PixelRect {
                                x,
                                y,
                                width,
                                height,
                            };
                            let left = x.clamp(0, w as i32);
                            let top = y.clamp(0, h as i32);
                            let right =
                                (i64::from(x) + i64::from(width)).clamp(0, i64::from(w)) as i32;
                            let bottom =
                                (i64::from(y) + i64::from(height)).clamp(0, i64::from(h)) as i32;
                            assert_eq!(
                                r.clamped(w, h),
                                PixelRect {
                                    x: left,
                                    y: top,
                                    width: right - left,
                                    height: bottom - top
                                },
                                "{r:?} on {w}x{h}"
                            );
                            let actual = crop(&src, &r);
                            assert_image(&output_image(&src, Some(r)), &actual);
                            if right <= left || bottom <= top {
                                assert_image(&actual, &src);
                            } else {
                                assert_mapping(
                                    &actual,
                                    &src,
                                    (right - left) as u32,
                                    (bottom - top) as u32,
                                    |dx, dy| (left as u32 + dx, top as u32 + dy),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn transforms_all_small_images() {
    for w in 1..=4 {
        for h in 1..=4 {
            let src = image(w, h);
            let horizontal = flip_horizontal(&src);
            assert_mapping(&horizontal, &src, w, h, |x, y| (w - 1 - x, y));
            assert_image(&flip_horizontal(&horizontal), &src);
            let vertical = flip_vertical(&src);
            assert_mapping(&vertical, &src, w, h, |x, y| (x, h - 1 - y));
            assert_image(&flip_vertical(&vertical), &src);
            let clockwise = rotate_90(&src);
            assert_mapping(&clockwise, &src, h, w, |x, y| (y, h - 1 - x));
            assert_image(&rotate_90(&rotate_90(&rotate_90(&clockwise))), &src);
            let half = rotate_180(&src);
            assert_mapping(&half, &src, w, h, |x, y| (w - 1 - x, h - 1 - y));
            assert_image(&rotate_180(&half), &src);
            assert_mapping(&rotate_270(&src), &src, h, w, |x, y| (w - 1 - y, x));
            assert_image(&rotate_arbitrary(&src, 0.0), &src);

            for dst_w in 0..=5 {
                for dst_h in 0..=5 {
                    let dst = resize(&src, dst_w, dst_h).unwrap();
                    if dst_w == 0 || dst_h == 0 {
                        assert_image(&dst, &src);
                    } else {
                        assert_eq!((dst.width, dst.height), (dst_w, dst_h));
                        assert_eq!(dst.data.len(), (dst_w * dst_h * 4) as usize);
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "既知の反例: 矩形端の加算が i32 の範囲を超える"]
fn clamp_signed_extremes() {
    let extremes = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
    for dimension in 1..=4 {
        for start in extremes {
            for span in extremes {
                let rect = PixelRect {
                    x: start,
                    y: start,
                    width: span,
                    height: span,
                };
                let left = start.clamp(0, dimension);
                let right =
                    (i64::from(start) + i64::from(span)).clamp(0, i64::from(dimension)) as i32;
                let expected = PixelRect {
                    x: left,
                    y: left,
                    width: right - left,
                    height: right - left,
                };
                assert_eq!(
                    rect.clamped(dimension as u32, dimension as u32),
                    expected,
                    "{rect:?}"
                );
            }
        }
    }
}

fn assert_arbitrary_angle(src: &DecodedImage, angle: i32) {
    let (sine, cosine) = match angle {
        0 => (0.0, 1.0),
        90 => (1.0, 0.0),
        180 => (0.0, -1.0),
        270 => (-1.0, 0.0),
        _ => f64::from(angle).to_radians().sin_cos(),
    };
    let (w, h) = (f64::from(src.width), f64::from(src.height));
    let left = (cosine * w).min(0.0) + (-sine * h).min(0.0);
    let top = (sine * w).min(0.0) + (cosine * h).min(0.0);
    let width = (cosine.abs() * w + sine.abs() * h).ceil() as u32;
    let height = (sine.abs() * w + cosine.abs() * h).ceil() as u32;
    let actual = rotate_arbitrary(src, f64::from(angle));
    assert_eq!(
        (actual.width, actual.height),
        (width, height),
        "{angle}度、入力 {}x{}",
        src.width,
        src.height
    );
    assert_eq!(actual.data.len(), (width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let dx = left + f64::from(x);
            let dy = top + f64::from(y);
            let sx = dx * cosine + dy * sine;
            let sy = dy * cosine - dx * sine;
            let observed = pixel(&actual, x, y);
            if !(0.0..w).contains(&sx) || !(0.0..h).contains(&sy) {
                assert_eq!(observed, [0; 4], "{angle}度、画素 ({x}, {y})");
                continue;
            }
            let (ix, iy) = (sx as u32, sy as u32);
            let fx = sx.fract();
            let fy = sy.fract();
            let rx = (ix + 1).min(src.width - 1);
            let by = (iy + 1).min(src.height - 1);
            for (ch, value) in observed.iter().enumerate() {
                let a = f64::from(pixel(src, ix, iy)[ch]);
                let b = f64::from(pixel(src, rx, iy)[ch]);
                let c = f64::from(pixel(src, ix, by)[ch]);
                let d = f64::from(pixel(src, rx, by)[ch]);
                let upper = a + (b - a) * fx;
                let lower = c + (d - c) * fx;
                let expected = (upper + (lower - upper) * fy).round() as u8;
                assert!(
                    value.abs_diff(expected) <= 1,
                    "{angle}度、画素 ({x}, {y}) のチャネル {ch}: 実値 {value}、期待値 {expected}"
                );
            }
        }
    }
}

#[test]
fn arbitrary_rotation_small_oblique_angles() {
    for w in 1..=4 {
        for h in 1..=4 {
            let src = image(w, h);
            for angle in [1, 45, 89] {
                assert_arbitrary_angle(&src, angle);
            }
        }
    }
}

#[test]
#[ignore = "既知の反例候補: 直角の三角関数の誤差により外接寸法や端の画素が変わる"]
fn arbitrary_rotation_small_right_angles() {
    for w in 1..=4 {
        for h in 1..=4 {
            let src = image(w, h);
            for angle in [0, 90, 180, 270] {
                assert_arbitrary_angle(&src, angle);
            }
        }
    }
}

#[test]
#[ignore = "反例候補: 横長画像の回転で出力のバイト数が u32 の範囲を超える"]
fn rotation_size_has_room_for_decoder_boundary() {
    // 標準デコーダの非厳格な既定の確保上限を RGBA8 に換算する。
    // Susie や PDF の読み込みを含む、全デコーダ共通の上限ではない。
    let max_rgba_pixels = ::image::Limits::default().max_alloc.unwrap() / 4;
    let width = max_rgba_pixels as u32;
    let height = 1_u32;
    let f = 45_f64.to_radians();
    let rotated_width = (f.cos() * f64::from(width) + f.sin() * f64::from(height)).ceil() as u32;
    let rotated_height = (f.sin() * f64::from(width) + f.cos() * f64::from(height)).ceil() as u32;
    let reference = u128::from(rotated_width) * u128::from(rotated_height) * 4;
    assert_eq!(
        rotation_byte_offset(rotated_width, 0, rotated_height) as u128,
        reference,
        "回転後 {rotated_width}x{rotated_height} のバイト数"
    );
    assert_eq!(
        rotation_byte_offset(rotated_width, rotated_width - 1, rotated_height - 1) as u128,
        reference - 4,
        "回転後の最終画素のバイト添字"
    );
}

#[test]
fn crop_size_and_source_offsets_at_decoder_boundary() {
    let max_pixels = (::image::Limits::default().max_alloc.unwrap() / 4) as usize;
    for (width, height) in [(1, max_pixels), (max_pixels, 1), (1024, max_pixels / 1024)] {
        let byte_count = (width as u128) * (height as u128) * 4;
        assert_eq!(crop_byte_offset(width, 0, height) as u128, byte_count);
        assert_eq!(
            crop_byte_offset(width, width - 1, height - 1) as u128,
            byte_count - 4
        );
    }
}
