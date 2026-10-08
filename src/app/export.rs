//! 表示画像と選択範囲のファイル出力

use super::AppWindow;
use anyhow::{Context as _, Result};
use std::path::Path;

impl AppWindow {
    /// 画像を指定フォーマットで保存する
    pub(super) fn export_image(&mut self, format: ExportFormat) {
        if self.document.current_image().is_none() {
            return;
        }
        let (default_stem, source_dir) = self.document.current_source().map_or_else(
            || ("image".to_string(), None),
            |s| (s.default_save_stem(), s.parent_dir().map(Path::to_path_buf)),
        );
        let initial_dir = self.file_operation_directory.initial(source_dir.as_deref());
        let default_name = format!("{default_stem}.{}", format.extension());

        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                crate::ui::file_dialog::save_file_dialog(
                    hwnd,
                    crate::ui::file_dialog::SaveFileDialogParams {
                        default_name: &default_name,
                        filter_name: format.filter_name(),
                        filter_ext: format.filter_spec(),
                        default_ext: format.extension(),
                        initial_dir: initial_dir.as_deref(),
                        ..Default::default()
                    },
                )
            },
            move |app, result| {
                let Some(save_path) = app.take_success("保存ダイアログの表示", result)
                else {
                    return;
                };
                let result = app.write_current_image(format, &save_path);
                if app.take_success("画像の出力", result.map(Some)).is_some() {
                    app.file_operation_directory.remember_file(&save_path);
                }
            },
        );
    }

    /// 表示中の画像 (選択範囲があればその範囲) を指定形式でファイルへ保存する
    pub(super) fn write_current_image(&self, format: ExportFormat, path: &Path) -> Result<()> {
        let img = self
            .document
            .current_image()
            .context("出力する画像がありません")?;
        let target = crate::filter::transform::output_image(img, self.selection.current_rect());
        write_image_to_path(target.width, target.height, &target.data, format, path)
    }
}
/// 画像保存フォーマット。各バリアントが拡張子・フィルタ表示・`image::ImageFormat`
/// を一元管理する。`Action::Export*` から `export_image` に渡される。
#[derive(Copy, Clone)]
pub(super) enum ExportFormat {
    Png,
    Jpg,
    Bmp,
}

impl ExportFormat {
    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Bmp => "bmp",
        }
    }

    fn filter_name(self) -> &'static str {
        match self {
            Self::Png => "PNG画像",
            Self::Jpg => "JPEG画像",
            Self::Bmp => "BMP画像",
        }
    }

    fn filter_spec(self) -> &'static str {
        match self {
            Self::Png => "*.png",
            Self::Jpg => "*.jpg",
            Self::Bmp => "*.bmp",
        }
    }

    fn image_format(self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpg => image::ImageFormat::Jpeg,
            Self::Bmp => image::ImageFormat::Bmp,
        }
    }
}

/// RGBA バッファを指定パスへ指定フォーマットで保存する。
///
/// `image::ImageBuffer<Rgba<u8>, _>` を直接エンコードすると、JPEG エンコーダ
/// が RGBA を受け付けず色型エラーで失敗する。`DynamicImage` を経由することで `image`
/// crate 側が必要な色変換 (RGBA→RGB 等) を自動で行う。フォーマットを引数で明示する
/// ため、保存先パスの拡張子有無に依存しない。
pub(super) fn write_image_to_path(
    width: u32,
    height: u32,
    rgba: &[u8],
    format: ExportFormat,
    path: &Path,
) -> Result<()> {
    let img_buf = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow::anyhow!("画像バッファの作成に失敗しました"))?;
    let dynamic = image::DynamicImage::ImageRgba8(img_buf);
    crate::shell::file_operations::save_atomic(path, |file| {
        dynamic
            .write_to(file, format.image_format())
            .context("画像の保存に失敗しました")
    })
}
