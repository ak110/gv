use anyhow::{Result, bail};
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::System::Memory::{LocalLock, LocalSize, LocalUnlock};

use crate::image::DecodedImage;

pub fn to_ansi(s: &str) -> Vec<u8> {
    use windows::Win32::Globalization::{CP_ACP, WC_NO_BEST_FIT_CHARS, WideCharToMultiByte};

    let wide: Vec<u16> = s.encode_utf16().collect();
    if wide.is_empty() {
        return vec![0];
    }

    // SAFETY: Win32 WideCharToMultiByte の規定通り、まず len 取得呼び出しで必要バイト数を確認し、
    // その +1 サイズの buf を渡してエンコードする。出力先バッファは buf スライスで境界が保証される。
    unsafe {
        let len = WideCharToMultiByte(CP_ACP, WC_NO_BEST_FIT_CHARS, &wide, None, None, None);
        if len == 0 {
            return vec![0];
        }

        let mut buf = vec![0u8; len as usize + 1];
        WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            &wide,
            Some(&mut buf),
            None,
            None,
        );
        if !buf.is_empty() && *buf.last().unwrap() != 0 {
            buf.push(0);
        }
        buf
    }
}

pub fn from_ansi(bytes: &[u8]) -> String {
    use windows::Win32::Globalization::{CP_ACP, MB_PRECOMPOSED, MultiByteToWideChar};

    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let ansi = &bytes[..end];
    if ansi.is_empty() {
        return String::new();
    }

    // SAFETY: Win32 MultiByteToWideChar の規定通り、まず wlen 取得呼び出しで必要ワイド文字数を
    // 確認し、その分の Vec<u16> を渡してデコードする。
    unsafe {
        let wlen = MultiByteToWideChar(CP_ACP, MB_PRECOMPOSED, ansi, None);
        if wlen == 0 {
            return String::new();
        }

        let mut wide = vec![0u16; wlen as usize];
        MultiByteToWideChar(CP_ACP, MB_PRECOMPOSED, ansi, Some(&mut wide));

        String::from_utf16_lossy(&wide)
    }
}

pub struct HLocalGuard(pub HLOCAL);

impl Drop for HLocalGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                windows::Win32::Foundation::LocalFree(Some(self.0));
            }
        }
    }
}

pub fn hlocal_from_isize(val: isize) -> HLOCAL {
    HLOCAL(val as *mut _)
}

/// DIB (Device Independent Bitmap) をRGBA画像に変換する
/// Susieプラグインの GetPicture が返す BITMAPINFO + ピクセルデータを処理
/// 対応: 1bit, 4bit, 8bit, 24bit, 32bit (bottom-up/top-down)
///
/// # SAFETY (関数全体)
///
/// 呼び出し側は以下を保証すること:
/// - `bmi_ptr` が `BitmapInfoHeader` (40 バイト) 以上のメモリ領域を指す。
///   bit_count <= 8 のときは続けて `colors_used * 4` バイトのパレット領域があること。
/// - `bits_ptr` が `stride * abs_height` バイト以上のメモリ領域を指し、関数の実行中
///   解放されないこと (Susie プラグインが GetPicture で確保したメモリの所有期間内)。
///
/// 関数内の各 `unsafe` ブロックは上記前提のもと、`width / height / bit_count` から
/// 計算した `src_row` と `x * bytes_per_pixel` のオフセットでのみメモリを参照する。
pub fn dib_to_rgba(bmi_ptr: *const u8, bits_ptr: *const u8) -> Result<DecodedImage> {
    if bmi_ptr.is_null() || bits_ptr.is_null() {
        bail!("DIBポインタがnull");
    }

    #[allow(clippy::cast_ptr_alignment)]
    let bih = unsafe { &*bmi_ptr.cast::<BitmapInfoHeader>() };
    let width = bih.bi_width;
    let height = bih.bi_height;
    let bit_count = bih.bi_bit_count;

    if width <= 0 {
        bail!("無効な画像幅: {width}");
    }
    let abs_height = height.unsigned_abs();
    let is_bottom_up = height > 0;

    let w = width as u32;
    let h = abs_height;

    let palette = if bit_count <= 8 {
        let colors_used = if bih.bi_clr_used > 0 {
            bih.bi_clr_used as usize
        } else {
            1 << bit_count
        };
        let palette_ptr = unsafe { bmi_ptr.add(std::mem::size_of::<BitmapInfoHeader>()) };
        let mut pal = Vec::with_capacity(colors_used);
        for i in 0..colors_used {
            let entry = unsafe { palette_ptr.add(i * 4) };
            let b = unsafe { *entry };
            let g = unsafe { *entry.add(1) };
            let r = unsafe { *entry.add(2) };
            pal.push((r, g, b, 255u8));
        }
        pal
    } else {
        Vec::new()
    };

    let stride = (width as usize * bit_count as usize).div_ceil(32) * 4;

    let mut rgba = vec![0u8; w as usize * h as usize * 4];

    for y in 0..h {
        let src_y = if is_bottom_up { h - 1 - y } else { y };
        let src_row = unsafe { bits_ptr.add(src_y as usize * stride) };
        let dst_offset = y as usize * w as usize * 4;

        match bit_count {
            32 => {
                for x in 0..w as usize {
                    let src = unsafe { src_row.add(x * 4) };
                    let b = unsafe { *src };
                    let g = unsafe { *src.add(1) };
                    let r = unsafe { *src.add(2) };
                    let a = unsafe { *src.add(3) };
                    let dst = dst_offset + x * 4;
                    rgba[dst] = r;
                    rgba[dst + 1] = g;
                    rgba[dst + 2] = b;
                    rgba[dst + 3] = a;
                }
            }
            24 => {
                for x in 0..w as usize {
                    let src = unsafe { src_row.add(x * 3) };
                    let b = unsafe { *src };
                    let g = unsafe { *src.add(1) };
                    let r = unsafe { *src.add(2) };
                    let dst = dst_offset + x * 4;
                    rgba[dst] = r;
                    rgba[dst + 1] = g;
                    rgba[dst + 2] = b;
                    rgba[dst + 3] = 255;
                }
            }
            8 => {
                for x in 0..w as usize {
                    let idx = unsafe { *src_row.add(x) } as usize;
                    let (r, g, b, a) = palette.get(idx).copied().unwrap_or((0, 0, 0, 255));
                    let dst = dst_offset + x * 4;
                    rgba[dst] = r;
                    rgba[dst + 1] = g;
                    rgba[dst + 2] = b;
                    rgba[dst + 3] = a;
                }
            }
            4 => {
                for x in 0..w as usize {
                    let byte = unsafe { *src_row.add(x / 2) };
                    let idx = if x % 2 == 0 {
                        (byte >> 4) as usize
                    } else {
                        (byte & 0x0F) as usize
                    };
                    let (r, g, b, a) = palette.get(idx).copied().unwrap_or((0, 0, 0, 255));
                    let dst = dst_offset + x * 4;
                    rgba[dst] = r;
                    rgba[dst + 1] = g;
                    rgba[dst + 2] = b;
                    rgba[dst + 3] = a;
                }
            }
            1 => {
                for x in 0..w as usize {
                    let byte = unsafe { *src_row.add(x / 8) };
                    let bit = (byte >> (7 - (x % 8))) & 1;
                    let idx = bit as usize;
                    let (r, g, b, a) = palette.get(idx).copied().unwrap_or((0, 0, 0, 255));
                    let dst = dst_offset + x * 4;
                    rgba[dst] = r;
                    rgba[dst + 1] = g;
                    rgba[dst + 2] = b;
                    rgba[dst + 3] = a;
                }
            }
            _ => {
                bail!("未対応のビット深度: {bit_count}");
            }
        }
    }

    Ok(DecodedImage {
        data: rgba,
        width: w,
        height: h,
    })
}

#[repr(C)]
#[allow(clippy::struct_field_names)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

pub fn with_local_lock<F, R>(handle: HLOCAL, f: F) -> R
where
    F: FnOnce(*mut core::ffi::c_void) -> R,
{
    let ptr = unsafe { LocalLock(handle) };
    let result = f(ptr);
    unsafe {
        let _ = LocalUnlock(handle);
    }
    result
}

pub fn local_size(handle: HLOCAL) -> usize {
    unsafe { LocalSize(handle) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_info_header_size() {
        use std::mem::{offset_of, size_of};

        assert_eq!(size_of::<BitmapInfoHeader>(), 40);
        assert_eq!(offset_of!(BitmapInfoHeader, bi_width), 4);
        assert_eq!(offset_of!(BitmapInfoHeader, bi_height), 8);
        assert_eq!(offset_of!(BitmapInfoHeader, bi_bit_count), 14);
        assert_eq!(offset_of!(BitmapInfoHeader, bi_compression), 16);
        assert_eq!(offset_of!(BitmapInfoHeader, bi_clr_used), 32);
    }

    #[test]
    fn ansi_roundtrip_ascii() {
        let original = "hello.jpg";
        let ansi = to_ansi(original);
        let back = from_ansi(&ansi);
        assert_eq!(back, original);
    }

    #[test]
    fn from_ansi_empty() {
        assert_eq!(from_ansi(&[0]), "");
        assert_eq!(from_ansi(&[]), "");
    }

    #[test]
    fn dib_to_rgba_24bit_2x2() {
        let bih = BitmapInfoHeader {
            bi_size: 40,
            bi_width: 2,
            bi_height: 2,
            bi_planes: 1,
            bi_bit_count: 24,
            bi_compression: 0,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        let bih_bytes = unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(&bih).cast::<u8>(),
                std::mem::size_of::<BitmapInfoHeader>(),
            )
        };

        #[rustfmt::skip]
        let bits: Vec<u8> = vec![
            0, 0, 255,  0, 255, 0,  0, 0,
            255, 0, 0,  255, 255, 255,  0, 0,
        ];

        let result = dib_to_rgba(bih_bytes.as_ptr(), bits.as_ptr()).unwrap();
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
        assert_eq!(&result.data[0..4], &[0, 0, 255, 255]);
        assert_eq!(&result.data[4..8], &[255, 255, 255, 255]);
        assert_eq!(&result.data[8..12], &[255, 0, 0, 255]);
        assert_eq!(&result.data[12..16], &[0, 255, 0, 255]);
    }

    #[test]
    fn dib_null_ptr_returns_error() {
        let result = dib_to_rgba(std::ptr::null(), std::ptr::null());
        assert!(result.is_err());
    }
}
