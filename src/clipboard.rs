//! Win32 Clipboard APIによるクリップボード操作

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_DIB;

use crate::image::DecodedImage;
use crate::util::to_wide;

const CF_UNICODETEXT: u32 = 13;

/// テキストをクリップボードにコピーする
pub fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> Result<()> {
    let wide = to_wide(text);
    let byte_len = wide.len() * 2;

    // SAFETY:
    // - hwnd は呼び出し元から渡された有効なウィンドウハンドル。
    // - GlobalAlloc(GMEM_MOVEABLE, byte_len) で確保した領域に GlobalLock で得たポインタへ
    //   wide (Vec<u16>) を byte_len バイトだけコピーする。コピー元・先ともに少なくとも
    //   byte_len バイトが確保済みで領域は重ならない。
    // - SetClipboardData 成功後はクリップボードがメモリの所有権を取るため呼び出し元では解放しない。
    unsafe {
        OpenClipboard(Some(hwnd)).context("クリップボードを開けません")?;
        let _ = EmptyClipboard();

        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len).context("GlobalAlloc失敗")?;
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            let _ = CloseClipboard();
            bail!("GlobalLock失敗");
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.cast::<u8>(), byte_len);
        let _ = GlobalUnlock(hmem);

        // SetClipboardDataに渡すHANDLEはGlobalAllocの戻り値をそのまま使う
        let handle = windows::Win32::Foundation::HANDLE(hmem.0.cast());
        let _ = SetClipboardData(CF_UNICODETEXT, Some(handle));
        let _ = CloseClipboard();
    }
    Ok(())
}

/// 画像をクリップボードにコピーする（CF_DIB形式）
pub fn copy_image_to_clipboard(hwnd: HWND, image: &DecodedImage) -> Result<()> {
    let header_size: usize = 40;
    let row_stride = (image.width as usize * 3).div_ceil(4) * 4;
    let pixel_size = row_stride * image.height as usize;
    let total_size = header_size + pixel_size;

    let mut dib = vec![0u8; total_size];

    // BITMAPINFOHEADER
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&image.width.to_le_bytes());
    dib[8..12].copy_from_slice(&image.height.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&24u16.to_le_bytes());

    // RGBA → BGR (上下反転)
    for y in 0..image.height as usize {
        let src_row = y * image.width as usize * 4;
        let dst_row = header_size + (image.height as usize - 1 - y) * row_stride;
        for x in 0..image.width as usize {
            let src = src_row + x * 4;
            let dst = dst_row + x * 3;
            dib[dst] = image.data[src + 2]; // B
            dib[dst + 1] = image.data[src + 1]; // G
            dib[dst + 2] = image.data[src]; // R
        }
    }

    // SAFETY:
    // - dib は total_size バイトの Vec で初期化済み。GlobalAlloc(GMEM_MOVEABLE, total_size) で
    //   確保した領域に同じバイト数だけコピーするので両側のバウンドは満たされる。
    // - SetClipboardData 成功後はクリップボードがメモリの所有権を取るため呼び出し元では解放しない。
    unsafe {
        OpenClipboard(Some(hwnd)).context("クリップボードを開けません")?;
        let _ = EmptyClipboard();

        let hmem = GlobalAlloc(GMEM_MOVEABLE, total_size).context("GlobalAlloc失敗")?;
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            let _ = CloseClipboard();
            bail!("GlobalLock失敗");
        }
        std::ptr::copy_nonoverlapping(dib.as_ptr(), ptr.cast::<u8>(), total_size);
        let _ = GlobalUnlock(hmem);

        let handle = windows::Win32::Foundation::HANDLE(hmem.0.cast());
        let _ = SetClipboardData(CF_DIB.0 as u32, Some(handle));
        let _ = CloseClipboard();
    }
    Ok(())
}

/// クリップボードから画像を取得する（CF_DIB形式）
pub fn paste_image_from_clipboard(hwnd: HWND) -> Result<Option<DecodedImage>> {
    // SAFETY:
    // - GetClipboardData の戻り値 HGLOBAL に GlobalLock してから読み取る。GlobalSize で
    //   実バイト数を確認し、required_size を計算してバウンドチェック後にのみアクセスする。
    // - DIB ヘッダは pack されていないアライメントの可能性があるため `ptr::read_unaligned` で
    //   読み取る (フィールドはそれぞれ 4/4/2 バイト境界に並ばない場合がある)。
    // - ピクセルデータの読み込みも src_row + x*bytes_per_pixel が required_size 内に収まる
    //   ことを上で検証済みなので、`*src.add(...)` は確保領域内。
    unsafe {
        OpenClipboard(Some(hwnd)).context("クリップボードを開けません")?;

        let handle = GetClipboardData(CF_DIB.0 as u32);
        let Ok(handle) = handle else {
            let _ = CloseClipboard();
            return Ok(None);
        };

        // HANDLEをHGLOBALとして扱う
        let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0.cast());
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = CloseClipboard();
            bail!("GlobalLock失敗");
        }

        let data_size = GlobalSize(hglobal);
        if data_size < 40 {
            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();
            bail!("DIBデータが不正（サイズ不足）");
        }

        let header: *const u8 = ptr.cast();
        let width = i32::from_le_bytes(std::ptr::read_unaligned(header.add(4).cast::<[u8; 4]>()));
        let height = i32::from_le_bytes(std::ptr::read_unaligned(header.add(8).cast::<[u8; 4]>()));
        let bit_count =
            u16::from_le_bytes(std::ptr::read_unaligned(header.add(14).cast::<[u8; 2]>()));

        let abs_height = height.unsigned_abs();
        let top_down = height < 0;

        if bit_count != 24 && bit_count != 32 {
            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();
            bail!("未対応のDIBビット深度: {bit_count}");
        }

        let bi_size = u32::from_le_bytes(std::ptr::read_unaligned(header.cast::<[u8; 4]>()));
        let bi_compression =
            u32::from_le_bytes(std::ptr::read_unaligned(header.add(16).cast::<[u8; 4]>()));

        if bi_size < 40 {
            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();
            bail!("DIBヘッダサイズが不正: {bi_size}");
        }

        let bytes_per_pixel = (bit_count / 8) as usize;
        let src_row_stride = (width as usize * bytes_per_pixel).div_ceil(4) * 4;
        let pixel_offset = compute_dib_pixel_offset(bi_size, bi_compression);

        // ピクセルデータが収まるか検証
        let required_size =
            pixel_offset.saturating_add(src_row_stride.saturating_mul(abs_height as usize));
        if required_size > data_size {
            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();
            bail!("DIBデータが不正（ピクセルデータ不足）");
        }

        let mut rgba = vec![0u8; width as usize * abs_height as usize * 4];

        for y in 0..abs_height as usize {
            let src_y = if top_down {
                y
            } else {
                abs_height as usize - 1 - y
            };
            let src_row = pixel_offset + src_y * src_row_stride;
            let dst_row = y * width as usize * 4;

            for x in 0..width as usize {
                let src = header.add(src_row + x * bytes_per_pixel);
                let dst = dst_row + x * 4;
                rgba[dst] = *src.add(2); // R
                rgba[dst + 1] = *src.add(1); // G
                rgba[dst + 2] = *src; // B
                rgba[dst + 3] = if bit_count == 32 { *src.add(3) } else { 255 };
            }
        }

        let _ = GlobalUnlock(hglobal);
        let _ = CloseClipboard();

        Ok(Some(DecodedImage {
            data: rgba,
            width: width as u32,
            height: abs_height,
        }))
    }
}

/// DIBヘッダのbiSizeとbiCompressionからピクセルデータの開始オフセットを計算する。
/// BI_BITFIELDSやBI_ALPHABITFIELDSの場合、カラーマスクがヘッダ直後に付加される。
fn compute_dib_pixel_offset(bi_size: u32, bi_compression: u32) -> usize {
    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;

    if bi_size == 40 {
        match bi_compression {
            BI_BITFIELDS => 40 + 12,      // 3つのDWORDカラーマスク
            BI_ALPHABITFIELDS => 40 + 16, // 4つのDWORDカラーマスク
            _ => 40,
        }
    } else {
        // BITMAPV4HEADER(108), BITMAPV5HEADER(124) 等はヘッダにマスクを含む
        bi_size as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_dib_pixel_offset_bi_rgb() {
        assert_eq!(compute_dib_pixel_offset(40, 0), 40);
    }

    #[test]
    fn test_compute_dib_pixel_offset_bi_bitfields() {
        assert_eq!(compute_dib_pixel_offset(40, 3), 52);
    }

    #[test]
    fn test_compute_dib_pixel_offset_bi_alphabitfields() {
        assert_eq!(compute_dib_pixel_offset(40, 6), 56);
    }

    #[test]
    fn test_compute_dib_pixel_offset_v4_header() {
        assert_eq!(compute_dib_pixel_offset(108, 0), 108);
        assert_eq!(compute_dib_pixel_offset(108, 3), 108);
    }

    #[test]
    fn test_compute_dib_pixel_offset_v5_header() {
        assert_eq!(compute_dib_pixel_offset(124, 0), 124);
        assert_eq!(compute_dib_pixel_offset(124, 3), 124);
    }
}
