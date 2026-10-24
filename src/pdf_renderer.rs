//! PDFページレンダリング
//!
//! Windows.Data.Pdf API を使ってPDFの各ページを画像にレンダリングする。
//! WinRT APIはスレッドセーフではないため、呼び出しごとにPdfDocumentを新規オープンする。
//!
//! ページの描画には`PdfPage::RenderToStreamAsync`系を使わず、gvが作成した
//! Direct3D 11デバイスへ`IPdfRendererNative::RenderPageToSurface`で描画する。
//! デバイスは描画スレッドで作成し、同じスレッドで描画を終えた時点で解放する。
//! `RenderToStreamAsync`系はOS部品（Windows.Data.Pdf）内部の静的オブジェクトが
//! Direct3Dデバイスを保持し、プロセス終了時のDLL解放でそのデバイスを破棄する。
//! WARPデバイスはこの破棄で停止済みのスレッドプールへ処理を投げるため、
//! 全処理の成功後でもプロセスが0xc000000d（STATUS_INVALID_PARAMETER）で異常終了する。
//! OS部品の静的オブジェクトへデバイスの寿命を委ねないことで、終了時の破棄を避ける。

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use windows::Data::Pdf::{PdfDocument, PdfPage};
use windows::Storage::StorageFile;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::WinRT::Pdf::{PDF_RENDER_PARAMS, PdfCreateRenderer};
use windows::core::{IUnknown, Interface as _};

use crate::image::DecodedImage;

/// レンダリング解像度スケール (150 DPI / 96 DPI ≈ 1.5625)
const DPI_SCALE: f64 = 1.5625;

/// PDFのページ数を取得する (高速: ページデータはロードしない)
pub fn get_pdf_page_count(pdf_path: &Path) -> Result<u32> {
    let clean_path = crate::util::strip_extended_length_prefix(pdf_path);
    let path_str = clean_path.to_str().unwrap_or("");
    if path_str.is_empty() {
        anyhow::bail!("無効なパス: {}", pdf_path.display());
    }

    let hstring = windows::core::HSTRING::from(path_str);
    let file = StorageFile::GetFileFromPathAsync(&hstring)
        .context("StorageFile取得失敗")?
        .join()
        .context("StorageFile非同期取得失敗")?;

    let doc = PdfDocument::LoadFromFileAsync(&file)
        .context("PdfDocument読み込み失敗")?
        .join()
        .context("PdfDocument非同期読み込み失敗")?;

    doc.PageCount().context("ページ数取得失敗")
}

/// 単一PDFページをDecodedImageにレンダリングする (150 DPI)
pub fn render_pdf_page(pdf_path: &Path, page_index: u32) -> Result<DecodedImage> {
    let clean_path = crate::util::strip_extended_length_prefix(pdf_path);
    let path_str = clean_path.to_str().unwrap_or("");
    if path_str.is_empty() {
        anyhow::bail!("無効なパス: {}", pdf_path.display());
    }

    let hstring = windows::core::HSTRING::from(path_str);
    let file = StorageFile::GetFileFromPathAsync(&hstring)
        .context("StorageFile取得失敗")?
        .join()
        .context("StorageFile非同期取得失敗")?;

    let doc = PdfDocument::LoadFromFileAsync(&file)
        .context("PdfDocument読み込み失敗")?
        .join()
        .context("PdfDocument非同期読み込み失敗")?;

    let page_count = doc.PageCount().context("ページ数取得失敗")?;
    if page_index >= page_count {
        bail!("ページインデックスが範囲外: {page_index} (総ページ数: {page_count})");
    }

    let page = doc.GetPage(page_index).context("ページ取得失敗")?;
    let size = page.Size().context("ページサイズ取得失敗")?;

    // 150 DPIにスケール
    let render_width = (size.Width as f64 * DPI_SCALE) as u32;
    let render_height = (size.Height as f64 * DPI_SCALE) as u32;

    render_page_to_rgba(&page, render_width.max(1), render_height.max(1))
}

/// ハードウェアのDirect3D 11デバイスを作成し、作成できなければWARPデバイスを作成する
fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                // PDFレンダラーはBGRAのサーフェスへ描画するためBGRA対応を要求する
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        };
        if let (Ok(()), Some(device), Some(context)) = (created, device, context) {
            return Ok((device, context));
        }
    }
    bail!("Direct3Dデバイス作成失敗")
}

/// PDFページをgvが所有するDirect3Dデバイスで白背景へ描画し、RGBAで返す
fn render_page_to_rgba(page: &PdfPage, width: u32, height: u32) -> Result<DecodedImage> {
    let (device, context) = create_d3d_device()?;

    let target_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut target: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&raw const target_desc, None, Some(&raw mut target)) }
        .context("描画先テクスチャ作成失敗")?;
    let target = target.context("描画先テクスチャ作成失敗")?;

    // 描画先はCPUから読めないため、読み取り用のテクスチャへ複写してから読む
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..target_desc
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&raw const staging_desc, None, Some(&raw mut staging)) }
        .context("読み取り用テクスチャ作成失敗")?;
    let staging = staging.context("読み取り用テクスチャ作成失敗")?;

    let renderer = unsafe { PdfCreateRenderer(&device.cast::<IDXGIDevice>()?) }
        .context("PDFレンダラー作成失敗")?;
    let params = PDF_RENDER_PARAMS {
        // 既定値（全0）の矩形はページ全体を描画する
        SourceRect: D2D_RECT_F::default(),
        DestinationWidth: width,
        DestinationHeight: height,
        BackgroundColor: D2D_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        IgnoreHighContrast: false,
    };
    unsafe {
        renderer.RenderPageToSurface(
            &page.cast::<IUnknown>()?,
            &target.cast::<IDXGISurface>()?,
            POINT { x: 0, y: 0 },
            Some(&raw const params),
        )
    }
    .context("レンダリング失敗")?;

    unsafe { context.CopyResource(&staging, &target) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped)) }
        .context("描画結果の読み取り失敗")?;
    let row_bytes = width as usize * 4;
    let mut data = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        // SAFETY: Mapが成功したため、pDataはUnmapまで有効で、各行はRowPitchバイト間隔に並ぶ。
        // RowPitchは1行の画素のバイト数（width * 4）以上であり、y < heightの行の先頭から
        // row_bytesバイトはマップされた領域内に収まる。
        let row = unsafe {
            std::slice::from_raw_parts(
                mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                row_bytes,
            )
        };
        for px in row.chunks_exact(4) {
            data.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    unsafe { context.Unmap(&staging, 0) };

    Ok(DecodedImage {
        data,
        width,
        height,
    })
}

/// STAメインスレッドから安全にPDFページ数を取得する
/// WinRT非同期APIはSTAでブロッキング待ちするとデッドロックするため、
/// MTAスレッドに委譲して実行する。
pub fn get_pdf_page_count_safe(pdf_path: &Path) -> Result<u32> {
    let path = pdf_path.to_path_buf();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let result = get_pdf_page_count(&path);
        unsafe {
            CoUninitialize();
        }
        result
    })
    .join()
    .map_err(|_| anyhow::anyhow!("PDFスレッドがパニック"))?
}

/// STAメインスレッドから安全にPDFページをレンダリングする
pub fn render_pdf_page_safe(pdf_path: &Path, page_index: u32) -> Result<DecodedImage> {
    let path = pdf_path.to_path_buf();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let result = render_pdf_page(&path, page_index);
        unsafe {
            CoUninitialize();
        }
        result
    })
    .join()
    .map_err(|_| anyhow::anyhow!("PDFスレッドがパニック"))?
}
