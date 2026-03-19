//! 永続フィルタ（全画像に自動適用されるフィルタ）
//!
//! 「フィルタ」メニューで設定した操作を、全画像閲覧時に自動適用する。
//! フィルタ設定変更時はキャッシュを全無効化して再描画する。

use crate::filter;
use crate::image::DecodedImage;

/// 永続フィルタの操作種別
#[derive(Debug, Clone)]
pub enum FilterOperation {
    FlipHorizontal,
    FlipVertical,
    Rotate180,
    Rotate90CW,
    Rotate90CCW,
    // 色変換
    Levels { low: u8, high: u8 },
    Gamma { value: f64 },
    BrightnessContrast { brightness: i32, contrast: i32 },
    GrayscaleSimple,
    GrayscaleStrict,
    // フィルタ
    Blur,
    BlurStrong,
    Sharpen,
    SharpenStrong,
    GaussianBlur { radius: f64 },
    UnsharpMask { radius: f64 },
    MedianFilter,
    // 色操作
    InvertColors,
    ApplyAlpha,
}

/// 永続フィルタ設定
pub struct PersistentFilter {
    /// フィルタの有効/無効
    enabled: bool,
    /// 適用するフィルタ操作のリスト（順序通りに適用）
    operations: Vec<FilterOperation>,
    /// 設定変更時にインクリメントされる世代番号
    generation: u64,
}

impl PersistentFilter {
    pub fn new() -> Self {
        Self {
            enabled: false,
            operations: Vec::new(),
            generation: 0,
        }
    }

    #[allow(dead_code)] // 将来のメニューチェック状態表示で使用予定
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
        self.generation += 1;
    }

    #[allow(dead_code)] // 将来のキャッシュキー統合で使用予定
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[allow(dead_code)] // 将来のフィルタリスト表示UIで使用予定
    pub fn operations(&self) -> &[FilterOperation] {
        &self.operations
    }

    /// フィルタ操作を追加する
    pub fn add_operation(&mut self, op: FilterOperation) {
        self.operations.push(op);
        self.generation += 1;
    }

    /// 全操作をクリアする
    #[allow(dead_code)] // 将来のUI操作で使用予定
    pub fn clear_operations(&mut self) {
        self.operations.clear();
        self.generation += 1;
    }

    /// フィルタが有効な場合、画像にフィルタを適用して返す
    /// 無効またはフィルタがない場合はNoneを返す（元画像をそのまま使う）
    pub fn apply(&self, image: &DecodedImage) -> Option<DecodedImage> {
        if !self.enabled || self.operations.is_empty() {
            return None;
        }

        let mut result = DecodedImage {
            data: image.data.clone(),
            width: image.width,
            height: image.height,
        };

        for op in &self.operations {
            result = apply_operation(&result, op);
        }

        Some(result)
    }
}

/// 単一のフィルタ操作を適用する
fn apply_operation(image: &DecodedImage, op: &FilterOperation) -> DecodedImage {
    match op {
        FilterOperation::FlipHorizontal => filter::transform::flip_horizontal(image),
        FilterOperation::FlipVertical => filter::transform::flip_vertical(image),
        FilterOperation::Rotate180 => filter::transform::rotate_180(image),
        FilterOperation::Rotate90CW => filter::transform::rotate_90(image),
        FilterOperation::Rotate90CCW => filter::transform::rotate_270(image),
        FilterOperation::Levels { low, high } => {
            filter::brightness::levels(image, None, *low, *high)
        }
        FilterOperation::Gamma { value } => filter::brightness::gamma(image, None, *value),
        FilterOperation::BrightnessContrast {
            brightness,
            contrast,
        } => filter::brightness::brightness_contrast(image, None, *brightness, *contrast),
        FilterOperation::GrayscaleSimple => filter::color::grayscale_simple(image, None),
        FilterOperation::GrayscaleStrict => filter::color::grayscale_strict(image, None),
        FilterOperation::Blur => filter::blur::blur(image, None),
        FilterOperation::BlurStrong => filter::blur::blur_strong(image, None),
        FilterOperation::Sharpen => filter::sharpen::sharpen(image, None),
        FilterOperation::SharpenStrong => filter::sharpen::sharpen_strong(image, None),
        FilterOperation::GaussianBlur { radius } => {
            filter::blur::gaussian_blur(image, None, *radius)
        }
        FilterOperation::UnsharpMask { radius } => filter::blur::unsharp_mask(image, None, *radius),
        FilterOperation::MedianFilter => filter::blur::median_filter(image, None),
        FilterOperation::InvertColors => filter::color::invert_colors(image, None),
        FilterOperation::ApplyAlpha => filter::color::apply_alpha(image, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image() -> DecodedImage {
        DecodedImage {
            data: vec![100, 150, 200, 255],
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn disabled_returns_none() {
        let pf = PersistentFilter::new();
        assert!(pf.apply(&test_image()).is_none());
    }

    #[test]
    fn enabled_with_no_ops_returns_none() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        assert!(pf.apply(&test_image()).is_none());
    }

    #[test]
    fn enabled_with_ops_applies() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::InvertColors);
        let result = pf.apply(&test_image()).unwrap();
        assert_eq!(result.data[0], 155); // 255-100
    }

    #[test]
    fn generation_increments() {
        let mut pf = PersistentFilter::new();
        assert_eq!(pf.generation(), 0);
        pf.toggle_enabled();
        assert_eq!(pf.generation(), 1);
        pf.add_operation(FilterOperation::Blur);
        assert_eq!(pf.generation(), 2);
    }

    #[test]
    fn multiple_operations_chain() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::InvertColors);
        pf.add_operation(FilterOperation::InvertColors);
        // 2回反転 → 元に戻る
        let img = test_image();
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data[0], img.data[0]);
    }
}
