use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_PROPERTIES, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1Factory, ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::layout::{DrawRect, Layout};
use crate::image::DecodedImage;

/// Direct2D描画エンジン
pub struct D2DRenderer {
    #[allow(dead_code)]
    factory: ID2D1Factory,
    render_target: ID2D1HwndRenderTarget,
    layout: Layout,
    /// 現在キャッシュ中のD2Dビットマップとそのソースポインタ（同一画像の再描画を高速化）
    cached_bitmap: Option<(usize, ID2D1Bitmap)>,
}

// 背景色: ダークグレー (#333333)
const BG_COLOR: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F =
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
        r: 0.2,
        g: 0.2,
        b: 0.2,
        a: 1.0,
    };

impl D2DRenderer {
    pub fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .context("D2D1Factory作成失敗")?;

            let render_target = Self::create_render_target(&factory, hwnd)?;

            Ok(Self {
                factory,
                render_target,
                layout: Layout::new(),
                cached_bitmap: None,
            })
        }
    }

    unsafe fn create_render_target(
        factory: &ID2D1Factory,
        hwnd: HWND,
    ) -> Result<ID2D1HwndRenderTarget> {
        unsafe {
            let mut rc = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc)?;

            let size = D2D_SIZE_U {
                width: (rc.right - rc.left) as u32,
                height: (rc.bottom - rc.top) as u32,
            };

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES::default();
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: size,
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };

            factory
                .CreateHwndRenderTarget(&rt_props, &hwnd_props)
                .context("HwndRenderTarget作成失敗")
        }
    }

    /// ウィンドウリサイズ時に呼ぶ
    pub fn resize(&mut self, width: u32, height: u32) {
        let size = D2D_SIZE_U { width, height };
        unsafe {
            let _ = self.render_target.Resize(&size);
        }
    }

    /// 画像を描画する。imageがNoneなら背景のみ。
    pub fn draw(&mut self, image: Option<&DecodedImage>) {
        unsafe {
            self.render_target.BeginDraw();
            self.render_target.Clear(Some(&BG_COLOR));

            if let Some(img) = image
                && let Ok(bitmap) = self.get_or_create_bitmap(img)
            {
                let size = self.render_target.GetSize();
                let draw_rect =
                    self.layout
                        .calculate(img.width, img.height, size.width, size.height);
                self.draw_bitmap(&bitmap, &draw_rect);
            }

            // EndDrawのエラーはリカバリ不要（次フレームで再試行される）
            let _ = self.render_target.EndDraw(None, None);
        }
    }

    /// DecodedImageからD2Dビットマップを取得（キャッシュ付き）
    unsafe fn get_or_create_bitmap(&mut self, image: &DecodedImage) -> Result<ID2D1Bitmap> {
        // ソースデータのポインタで同一性を判定
        let key = image.data.as_ptr() as usize;
        if let Some((cached_key, ref bitmap)) = self.cached_bitmap
            && cached_key == key
        {
            return Ok(bitmap.clone());
        }

        let bitmap = unsafe { self.create_bitmap_from_image(image)? };
        self.cached_bitmap = Some((key, bitmap.clone()));
        Ok(bitmap)
    }

    /// RGBA画像データからD2Dビットマップを作成
    /// image crateはRGBA、D2DはBGRA前提なのでチャネル入れ替えが必要
    unsafe fn create_bitmap_from_image(&self, image: &DecodedImage) -> Result<ID2D1Bitmap> {
        // RGBA → BGRA変換
        let mut bgra_data = image.data.clone();
        for pixel in bgra_data.chunks_exact_mut(4) {
            pixel.swap(0, 2); // R ↔ B
        }

        let size = D2D_SIZE_U {
            width: image.width,
            height: image.height,
        };
        let pixel_format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let bitmap_props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: pixel_format,
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let pitch = image.width * 4;

        unsafe {
            self.render_target
                .CreateBitmap(
                    size,
                    Some(bgra_data.as_ptr() as *const _),
                    pitch,
                    &bitmap_props,
                )
                .context("D2Dビットマップ作成失敗")
        }
    }

    unsafe fn draw_bitmap(&self, bitmap: &ID2D1Bitmap, rect: &DrawRect) {
        unsafe {
            let dest = D2D_RECT_F {
                left: rect.x,
                top: rect.y,
                right: rect.x + rect.width,
                bottom: rect.y + rect.height,
            };
            self.render_target.DrawBitmap(
                bitmap,
                Some(&dest),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
    }

    #[allow(dead_code)]
    pub fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }
}

use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR;
