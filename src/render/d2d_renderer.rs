use anyhow::{Context as _, Result};
use fast_image_resize as fr;
use rayon::prelude::*;
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1BitmapBrush, ID2D1Factory, ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_EXTEND_MODE_WRAP};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::layout::{DrawRect, Layout};
use crate::config::DisplayConfig;
use crate::image::DecodedImage;
use crate::selection::{self, HANDLE_DRAW_SIZE, PixelRect};

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaBackground {
    White,
    Black,
    #[default]
    Checker,
}

struct PrescaledEntry {
    source_key: usize,
    width: u32,
    height: u32,
    bitmap: ID2D1Bitmap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOutcome {
    Drawn,
    TargetLost,
}

struct TargetResources {
    render_target: ID2D1HwndRenderTarget,
    cached_bitmap: Option<(usize, ID2D1Bitmap)>,
    prescaled: Option<PrescaledEntry>,
    checker_brush: Option<ID2D1BitmapBrush>,
}

struct FrameParams<'a> {
    image: Option<&'a DecodedImage>,
    selection_rect: Option<&'a PixelRect>,
    layout: &'a Layout,
    alpha_bg: AlphaBackground,
    draw_offset_x: f32,
}

struct FrameResult {
    draw_rect: Option<DrawRect>,
    content: Result<()>,
    end_draw: windows::core::Result<()>,
}

pub struct D2DRenderer {
    factory: ID2D1Factory,
    hwnd: HWND,
    target: Option<TargetResources>,
    layout: Layout,
    alpha_bg: AlphaBackground,
    draw_offset_x: f32,
    last_draw_rect: Option<DrawRect>,
    #[cfg(test)]
    injected_end_draw_error: Option<windows::core::Error>,
    #[cfg(test)]
    fail_next_target_creation: bool,
}

const BG_COLOR: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.2,
    g: 0.2,
    b: 0.2,
    a: 1.0,
};

impl D2DRenderer {
    pub fn new(hwnd: HWND, display_config: &DisplayConfig) -> Result<Self> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .context("D2D1Factory作成失敗")?;
        let layout = Layout::from_config(display_config.to_display_mode(), display_config.margin);

        let mut renderer = Self {
            factory,
            hwnd,
            target: None,
            layout,
            alpha_bg: display_config.alpha_background,
            draw_offset_x: 0.0,
            last_draw_rect: None,
            #[cfg(test)]
            injected_end_draw_error: None,
            #[cfg(test)]
            fail_next_target_creation: false,
        };
        renderer.target = Some(renderer.create_target_resources()?);
        Ok(renderer)
    }

    fn create_target_resources(&mut self) -> Result<TargetResources> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_target_creation) {
            anyhow::bail!("HwndRenderTarget作成失敗 (試験経路)");
        }
        let render_target = unsafe { Self::create_render_target(&self.factory, self.hwnd)? };
        Ok(TargetResources {
            render_target,
            cached_bitmap: None,
            prescaled: None,
            checker_brush: None,
        })
    }

    unsafe fn create_render_target(
        factory: &ID2D1Factory,
        hwnd: HWND,
    ) -> Result<ID2D1HwndRenderTarget> {
        unsafe {
            let mut rc = std::mem::zeroed();
            GetClientRect(hwnd, std::ptr::from_mut(&mut rc))?;

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
                .CreateHwndRenderTarget(
                    std::ptr::from_ref(&rt_props),
                    std::ptr::from_ref(&hwnd_props),
                )
                .context("HwndRenderTarget作成失敗")
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let Some(target) = &self.target else {
            return Ok(());
        };
        let size = D2D_SIZE_U { width, height };
        match unsafe { target.render_target.Resize(std::ptr::from_ref(&size)) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                self.target = None;
                Ok(())
            }
            Err(e) => Err(anyhow::Error::from(e).context("描画領域のリサイズに失敗しました")),
        }
    }

    pub fn draw(
        &mut self,
        image: Option<&DecodedImage>,
        selection_rect: Option<&PixelRect>,
    ) -> Result<DrawOutcome> {
        if self.target.is_none() {
            self.target = Some(self.create_target_resources()?);
        }
        let target = self.target.as_mut().expect("target was just ensured");
        let params = FrameParams {
            image,
            selection_rect,
            layout: &self.layout,
            alpha_bg: self.alpha_bg,
            draw_offset_x: self.draw_offset_x,
        };
        let frame = unsafe { target.draw_frame(&params) };
        self.last_draw_rect = frame.draw_rect;

        #[cfg(test)]
        let end_draw = match self.injected_end_draw_error.take() {
            Some(e) => Err(e),
            None => frame.end_draw,
        };
        #[cfg(not(test))]
        let end_draw = frame.end_draw;

        match end_draw {
            Ok(()) => {}
            Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                self.target = None;
                return Ok(DrawOutcome::TargetLost);
            }
            Err(e) => return Err(anyhow::Error::from(e).context("描画の確定に失敗しました")),
        }
        frame.content?;
        Ok(DrawOutcome::Drawn)
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn update_layout(&mut self, update: impl FnOnce(&mut Layout)) {
        update(&mut self.layout);
    }

    pub fn cycle_alpha_background(&mut self) {
        self.alpha_bg = match self.alpha_bg {
            AlphaBackground::White => AlphaBackground::Black,
            AlphaBackground::Black => AlphaBackground::Checker,
            AlphaBackground::Checker => AlphaBackground::White,
        };
    }

    pub fn set_draw_offset(&mut self, offset_x: f32) {
        self.draw_offset_x = offset_x;
    }

    pub fn last_draw_rect(&self) -> Option<&DrawRect> {
        self.last_draw_rect.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn simulate_target_loss_on_next_draw(&mut self) {
        self.injected_end_draw_error = Some(D2DERR_RECREATE_TARGET.into());
    }

    #[cfg(test)]
    pub(crate) fn fail_next_target_creation(&mut self) {
        self.fail_next_target_creation = true;
    }

    #[cfg(test)]
    pub(crate) fn target_snapshot(&self) -> Option<TargetSnapshot> {
        self.target.as_ref().map(|t| TargetSnapshot {
            has_bitmap: t.cached_bitmap.is_some() || t.prescaled.is_some(),
            has_checker_brush: t.checker_brush.is_some(),
        })
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetSnapshot {
    pub(crate) has_bitmap: bool,
    pub(crate) has_checker_brush: bool,
}

impl TargetResources {
    unsafe fn draw_frame(&mut self, params: &FrameParams<'_>) -> FrameResult {
        unsafe {
            self.render_target.BeginDraw();

            let size = self.render_target.GetSize();

            let has_clip = params.draw_offset_x > 0.0;
            if has_clip {
                let clip = D2D_RECT_F {
                    left: params.draw_offset_x,
                    top: 0.0,
                    right: size.width,
                    bottom: size.height,
                };
                self.render_target.PushAxisAlignedClip(
                    std::ptr::from_ref(&clip),
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
            }

            self.render_target.Clear(Some(&BG_COLOR));

            let mut draw_rect_out = None;
            let content = if let Some(img) = params.image {
                let avail_width = size.width - params.draw_offset_x;
                let mut draw_rect =
                    params
                        .layout
                        .calculate(img.width, img.height, avail_width, size.height);
                draw_rect.x += params.draw_offset_x;
                draw_rect_out = Some(draw_rect);
                self.draw_image(img, &draw_rect, params)
            } else {
                Ok(())
            };

            if has_clip {
                self.render_target.PopAxisAlignedClip();
            }

            let end_draw = self.render_target.EndDraw(None, None);
            FrameResult {
                draw_rect: draw_rect_out,
                content,
                end_draw,
            }
        }
    }

    unsafe fn draw_image(
        &mut self,
        img: &DecodedImage,
        draw_rect: &DrawRect,
        params: &FrameParams<'_>,
    ) -> Result<()> {
        unsafe {
            self.draw_alpha_background(draw_rect, params.alpha_bg)?;

            let target_w = (draw_rect.width as u32).max(1);
            let target_h = (draw_rect.height as u32).max(1);
            let bitmap = self.get_prescaled_bitmap(img, target_w, target_h)?;
            self.draw_bitmap(&bitmap, draw_rect);

            let scale = draw_rect.width / img.width as f32;
            if scale >= 8.0 {
                self.draw_pixel_grid(draw_rect, img.width, img.height, scale);
            }

            if let Some(sel_rect) = params.selection_rect {
                self.draw_selection_overlay(sel_rect, draw_rect, img.width, img.height);
            }
            Ok(())
        }
    }

    unsafe fn get_or_create_bitmap(&mut self, image: &DecodedImage) -> Result<ID2D1Bitmap> {
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

    unsafe fn get_prescaled_bitmap(
        &mut self,
        image: &DecodedImage,
        target_width: u32,
        target_height: u32,
    ) -> Result<ID2D1Bitmap> {
        let key = image.data.as_ptr() as usize;

        if let Some(ref entry) = self.prescaled
            && entry.source_key == key
            && entry.width == target_width
            && entry.height == target_height
        {
            return Ok(entry.bitmap.clone());
        }

        if target_width == image.width && target_height == image.height {
            return unsafe { self.get_or_create_bitmap(image) };
        }

        let scale = (f64::from(target_width) / f64::from(image.width))
            .min(f64::from(target_height) / f64::from(image.height));
        let resized_data = if scale >= 1.0 {
            fir_resize(
                &image.data,
                image.width,
                image.height,
                target_width,
                target_height,
                fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3),
            )?
        } else if scale >= 0.5 {
            fir_resize(
                &image.data,
                image.width,
                image.height,
                target_width,
                target_height,
                fr::ResizeAlg::Convolution(fr::FilterType::Mitchell),
            )?
        } else {
            fir_resize_multistage(
                &image.data,
                image.width,
                image.height,
                target_width,
                target_height,
            )?
        };

        let prescaled = DecodedImage {
            data: resized_data,
            width: target_width,
            height: target_height,
        };

        let bitmap = unsafe { self.create_bitmap_from_image(&prescaled)? };
        self.prescaled = Some(PrescaledEntry {
            source_key: key,
            width: target_width,
            height: target_height,
            bitmap: bitmap.clone(),
        });
        Ok(bitmap)
    }

    unsafe fn create_bitmap_from_image(&self, image: &DecodedImage) -> Result<ID2D1Bitmap> {
        let bgra_data = rgba_to_premultiplied_bgra(&image.data);

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

        // SAFETY: D2D は (pitch * height) バイトをコピーする。bgra_data は
        // rgba_to_premultiplied_bgra で `image.data.len()` (= width*height*4) バイトを返すため
        // pitch (= width*4) × height = bgra_data.len() を満たす。CreateBitmap は同期コピーで
        // 戻った時点で bgra_data の参照は不要になるため寿命の問題もない。
        unsafe {
            self.render_target
                .CreateBitmap(
                    size,
                    Some(bgra_data.as_ptr().cast::<core::ffi::c_void>()),
                    pitch,
                    std::ptr::from_ref(&bitmap_props),
                )
                .context("D2Dビットマップ作成失敗")
        }
    }

    unsafe fn draw_alpha_background(
        &mut self,
        rect: &DrawRect,
        alpha_bg: AlphaBackground,
    ) -> Result<()> {
        let dest = D2D_RECT_F {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width,
            bottom: rect.y + rect.height,
        };

        unsafe {
            let solid = |r: f32, g: f32, b: f32| D2D1_COLOR_F { r, g, b, a: 1.0 };
            match alpha_bg {
                AlphaBackground::White | AlphaBackground::Black => {
                    let color = if alpha_bg == AlphaBackground::White {
                        solid(1.0, 1.0, 1.0)
                    } else {
                        solid(0.0, 0.0, 0.0)
                    };
                    let brush = self
                        .render_target
                        .CreateSolidColorBrush(std::ptr::from_ref(&color), None)
                        .context("α背景ブラシ作成失敗")?;
                    self.render_target
                        .FillRectangle(std::ptr::from_ref(&dest), &brush);
                }
                AlphaBackground::Checker => {
                    let brush = self.ensure_checker_brush()?;
                    self.render_target
                        .FillRectangle(std::ptr::from_ref(&dest), &brush);
                }
            }
        }
        Ok(())
    }

    fn ensure_checker_brush(&mut self) -> Result<ID2D1BitmapBrush> {
        if let Some(brush) = &self.checker_brush {
            return Ok(brush.clone());
        }
        let brush = unsafe { self.create_checker_brush()? };
        self.checker_brush = Some(brush.clone());
        Ok(brush)
    }

    unsafe fn create_checker_brush(&self) -> Result<ID2D1BitmapBrush> {
        const TILE_SIZE: u32 = 16;
        const BLOCK_SIZE: u32 = 8;
        let mut pixels = vec![0u8; (TILE_SIZE * TILE_SIZE * 4) as usize];
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let offset = ((y * TILE_SIZE + x) * 4) as usize;
                let is_light = ((x / BLOCK_SIZE) + (y / BLOCK_SIZE)).is_multiple_of(2);
                let color: u8 = if is_light { 0xFF } else { 0xCC };
                pixels[offset] = color;
                pixels[offset + 1] = color;
                pixels[offset + 2] = color;
                pixels[offset + 3] = 0xFF;
            }
        }

        let size = D2D_SIZE_U {
            width: TILE_SIZE,
            height: TILE_SIZE,
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

        // SAFETY: CreateBitmapは (pitch * height) バイトを同期コピーする。pixelsは
        // (TILE_SIZE * TILE_SIZE * 4) バイト確保済みで、pitch (= TILE_SIZE * 4) × TILE_SIZE
        // = pixels.len() を満たす。戻り時点でpixelsの参照は不要。
        unsafe {
            let tile_bitmap = self
                .render_target
                .CreateBitmap(
                    size,
                    Some(pixels.as_ptr().cast::<core::ffi::c_void>()),
                    TILE_SIZE * 4,
                    std::ptr::from_ref(&bitmap_props),
                )
                .context("チェッカータイルビットマップ作成失敗")?;

            let brush_props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_WRAP,
                extendModeY: D2D1_EXTEND_MODE_WRAP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            self.render_target
                .CreateBitmapBrush(&tile_bitmap, Some(std::ptr::from_ref(&brush_props)), None)
                .context("チェッカーブラシ作成失敗")
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
                Some(std::ptr::from_ref(&dest)),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
    }

    unsafe fn draw_pixel_grid(
        &self,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
        scale: f32,
    ) {
        unsafe {
            let Ok(brush) = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 0.3,
                },
                None,
            ) else {
                return;
            };

            let half = 0.25;

            for x in 0..=img_width {
                let sx = draw_rect.x + x as f32 * scale;
                let rect = D2D_RECT_F {
                    left: sx - half,
                    top: draw_rect.y,
                    right: sx + half,
                    bottom: draw_rect.y + draw_rect.height,
                };
                self.render_target
                    .FillRectangle(std::ptr::from_ref(&rect), &brush);
            }

            for y in 0..=img_height {
                let sy = draw_rect.y + y as f32 * scale;
                let rect = D2D_RECT_F {
                    left: draw_rect.x,
                    top: sy - half,
                    right: draw_rect.x + draw_rect.width,
                    bottom: sy + half,
                };
                self.render_target
                    .FillRectangle(std::ptr::from_ref(&rect), &brush);
            }
        }
    }

    unsafe fn draw_selection_overlay(
        &self,
        sel_rect: &PixelRect,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) {
        unsafe {
            let (left, top) = selection::image_to_screen(
                sel_rect.x, sel_rect.y, draw_rect, img_width, img_height,
            );
            let (right, bottom) = selection::image_to_screen(
                sel_rect.right(),
                sel_rect.bottom(),
                draw_rect,
                img_width,
                img_height,
            );

            let rect_outer = D2D_RECT_F {
                left,
                top,
                right,
                bottom,
            };

            if let Ok(black_brush) = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.8,
                },
                None,
            ) {
                self.render_target.DrawRectangle(
                    std::ptr::from_ref(&rect_outer),
                    &black_brush,
                    2.0,
                    None,
                );
            }

            if let Ok(white_brush) = self.render_target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.9,
                },
                None,
            ) {
                self.render_target.DrawRectangle(
                    std::ptr::from_ref(&rect_outer),
                    &white_brush,
                    1.0,
                    None,
                );

                let handles = selection::handle_positions(sel_rect);
                let hs = HANDLE_DRAW_SIZE;
                for (_kind, hx, hy) in &handles {
                    let (sx, sy) =
                        selection::image_to_screen(*hx, *hy, draw_rect, img_width, img_height);
                    let handle_rect = D2D_RECT_F {
                        left: sx - hs,
                        top: sy - hs,
                        right: sx + hs,
                        bottom: sy + hs,
                    };
                    self.render_target
                        .FillRectangle(std::ptr::from_ref(&handle_rect), &white_brush);
                }
            }
        }
    }
}

fn rgba_to_premultiplied_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = rgba.to_vec();
    let row_bytes = 4 * 256;
    bgra.par_chunks_mut(row_bytes).for_each(|chunk| {
        for pixel in chunk.chunks_exact_mut(4) {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];
            let a = pixel[3] as u16;
            pixel[0] = ((b as u16 * a) / 255) as u8;
            pixel[1] = ((g as u16 * a) / 255) as u8;
            pixel[2] = ((r as u16 * a) / 255) as u8;
        }
    });
    bgra
}

fn fir_resize(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    resize_alg: fr::ResizeAlg,
) -> Result<Vec<u8>> {
    let src_image = fr::images::ImageRef::new(src_w, src_h, src, fr::PixelType::U8x4)
        .context("リサイズ用ソース画像作成失敗")?;
    let mut dst_image = fr::images::Image::new(dst_w, dst_h, fr::PixelType::U8x4);
    let options = fr::ResizeOptions::new().resize_alg(resize_alg);
    let mut resizer = fr::Resizer::new();
    resizer
        .resize(&src_image, &mut dst_image, &options)
        .context("画像リサイズ失敗")?;
    Ok(dst_image.into_vec())
}

fn fir_resize_multistage(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Result<Vec<u8>> {
    let mitchell = fr::ResizeAlg::Convolution(fr::FilterType::Mitchell);

    let mut current_data = src.to_vec();
    let mut cur_w = src_w;
    let mut cur_h = src_h;

    while cur_w > dst_w * 2 && cur_h > dst_h * 2 {
        let next_w = (cur_w / 2).max(dst_w);
        let next_h = (cur_h / 2).max(dst_h);
        current_data = fir_resize(&current_data, cur_w, cur_h, next_w, next_h, mitchell)?;
        cur_w = next_w;
        cur_h = next_h;
    }

    fir_resize(
        &current_data,
        cur_w,
        cur_h,
        dst_w,
        dst_h,
        fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::HiddenWindow;
    use crate::test_helpers::rect_tuple;
    use crate::test_helpers::translucent_image;

    #[test]
    fn recovers_after_target_loss_with_same_layout() {
        let window = HiddenWindow::new();
        let config = DisplayConfig {
            alpha_background: AlphaBackground::Checker,
            ..DisplayConfig::default()
        };
        let mut renderer = D2DRenderer::new(window.0, &config).unwrap();
        renderer.set_draw_offset(40.0);
        let image = translucent_image();
        let selection = PixelRect::from_two_points(1, 1, 5, 4);

        assert_eq!(
            renderer.draw(Some(&image), Some(&selection)).unwrap(),
            DrawOutcome::Drawn
        );
        let before = renderer.target_snapshot().expect("target exists");
        assert!(before.has_bitmap && before.has_checker_brush);
        let rect_before = rect_tuple(renderer.last_draw_rect().unwrap());

        renderer.simulate_target_loss_on_next_draw();
        assert_eq!(
            renderer.draw(Some(&image), Some(&selection)).unwrap(),
            DrawOutcome::TargetLost
        );
        assert!(renderer.target_snapshot().is_none());

        assert_eq!(
            renderer.draw(Some(&image), Some(&selection)).unwrap(),
            DrawOutcome::Drawn
        );
        let after = renderer.target_snapshot().expect("target recreated");
        assert!(after.has_bitmap && after.has_checker_brush);
        assert_eq!(rect_tuple(renderer.last_draw_rect().unwrap()), rect_before);
    }

    #[test]
    fn resize_without_target_is_deferred_to_next_draw() {
        let window = HiddenWindow::new();
        let mut renderer = D2DRenderer::new(window.0, &DisplayConfig::default()).unwrap();
        let image = translucent_image();
        renderer.simulate_target_loss_on_next_draw();
        assert_eq!(
            renderer.draw(Some(&image), None).unwrap(),
            DrawOutcome::TargetLost
        );
        renderer.resize(200, 100).unwrap();
        assert!(renderer.target_snapshot().is_none());
        assert_eq!(
            renderer.draw(Some(&image), None).unwrap(),
            DrawOutcome::Drawn
        );
    }

    #[test]
    fn fir_resize_shrink_output_size() {
        let src = [[200, 40, 80, 255].repeat(2), [40, 200, 80, 255].repeat(2)]
            .concat()
            .repeat(4);
        let result = fir_resize(
            &src,
            4,
            4,
            2,
            2,
            fr::ResizeAlg::Convolution(fr::FilterType::Mitchell),
        )
        .unwrap();
        assert_eq!(result.len(), 2 * 2 * 4);
        for row in result.chunks_exact(2 * 4) {
            let left = &row[..4];
            let right = &row[row.len() - 4..];
            assert!(
                i16::from(left[0]) - i16::from(left[1]) > 40
                    && i16::from(right[1]) - i16::from(right[0]) > 40
            );
            assert!(left[2].abs_diff(80) <= 1 && right[2].abs_diff(80) <= 1);
            assert_eq!((left[3], right[3]), (255, 255));
        }
    }

    #[test]
    fn fir_resize_enlarge_output_size() {
        let src = [200, 40, 80, 255, 40, 200, 80, 255].repeat(2);
        let result = fir_resize(
            &src,
            2,
            2,
            4,
            4,
            fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3),
        )
        .unwrap();
        assert_eq!(result.len(), 4 * 4 * 4);
        for row in result.chunks_exact(4 * 4) {
            let left = &row[..4];
            let right = &row[row.len() - 4..];
            assert!(
                i16::from(left[0]) - i16::from(left[1]) > 40
                    && i16::from(right[1]) - i16::from(right[0]) > 40
            );
            assert!(left[2].abs_diff(80) <= 1 && right[2].abs_diff(80) <= 1);
            assert_eq!((left[3], right[3]), (255, 255));
        }
    }

    #[test]
    fn fir_resize_uniform_shrink() {
        let src: Vec<u8> = [100, 150, 200, 255].repeat(16);
        let result = fir_resize(
            &src,
            4,
            4,
            2,
            2,
            fr::ResizeAlg::Convolution(fr::FilterType::Mitchell),
        )
        .unwrap();
        for pixel in result.chunks_exact(4) {
            assert_eq!(pixel, [100, 150, 200, 255]);
        }
    }

    #[test]
    fn fir_resize_alpha_no_halo() {
        let mut src = Vec::with_capacity(4 * 4 * 4);
        for y in 0..4u32 {
            for _ in 0..4u32 {
                if y < 2 {
                    src.extend_from_slice(&[255, 0, 0, 255]);
                } else {
                    src.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        let result = fir_resize(
            &src,
            4,
            4,
            2,
            2,
            fr::ResizeAlg::Convolution(fr::FilterType::Mitchell),
        )
        .unwrap();
        for pixel in result.chunks_exact(4) {
            let a = pixel[3] as u16;
            if a > 0 {
                assert!(
                    pixel[1] < 30 && pixel[2] < 30,
                    "ハロー検出: pixel={pixel:?}"
                );
            }
        }
    }

    #[test]
    fn fir_resize_multistage_output_size() {
        let src = [[200, 40, 80, 255].repeat(8), [40, 200, 80, 255].repeat(8)]
            .concat()
            .repeat(16);
        let result = fir_resize_multistage(&src, 16, 16, 2, 2).unwrap();
        assert_eq!(result.len(), 2 * 2 * 4);
        for row in result.chunks_exact(2 * 4) {
            let left = &row[..4];
            let right = &row[row.len() - 4..];
            assert!(
                i16::from(left[0]) - i16::from(left[1]) > 40
                    && i16::from(right[1]) - i16::from(right[0]) > 40
            );
            assert!(left[2].abs_diff(80) <= 1 && right[2].abs_diff(80) <= 1);
            assert_eq!((left[3], right[3]), (255, 255));
        }
    }

    #[test]
    fn fir_resize_multistage_uniform_color() {
        let src: Vec<u8> = [80, 120, 200, 255].repeat(32 * 32);
        let result = fir_resize_multistage(&src, 32, 32, 4, 4).unwrap();
        for pixel in result.chunks_exact(4) {
            assert!(
                (pixel[0] as i16 - 80).unsigned_abs() <= 1,
                "R: {}",
                pixel[0]
            );
            assert!(
                (pixel[1] as i16 - 120).unsigned_abs() <= 1,
                "G: {}",
                pixel[1]
            );
            assert!(
                (pixel[2] as i16 - 200).unsigned_abs() <= 1,
                "B: {}",
                pixel[2]
            );
            assert_eq!(pixel[3], 255);
        }
    }
}
