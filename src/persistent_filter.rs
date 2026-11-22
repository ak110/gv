use crate::filter;
use crate::filter_spec::FilterKind;
use crate::image::DecodedImage;
use crate::selection::PixelRect;

#[derive(Debug, Clone)]
pub enum FilterOperation {
    FlipHorizontal,
    FlipVertical,
    Rotate180,
    Rotate90CW,
    Rotate90CCW,
    Fill { red: u8, green: u8, blue: u8 },
    Levels { low: u8, high: u8 },
    Gamma { value: f64 },
    BrightnessContrast { brightness: i32, contrast: i32 },
    GrayscaleSimple,
    GrayscaleStrict,
    Blur,
    BlurStrong,
    Sharpen,
    SharpenStrong,
    Mosaic { size: u32 },
    GaussianBlur { radius: f64 },
    UnsharpMask { radius: f64 },
    MedianFilter,
    InvertColors,
    ApplyAlpha,
}

#[derive(Clone)]
pub struct PersistentFilter {
    enabled: bool,
    operations: Vec<FilterOperation>,
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

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
        self.generation += 1;
    }

    #[cfg(test)]
    pub fn operations(&self) -> &[FilterOperation] {
        &self.operations
    }

    pub fn has_operation(&self, kind: FilterKind) -> bool {
        self.operations.iter().any(|op| op.kind() == kind)
    }

    pub fn add_operation(&mut self, op: FilterOperation) {
        self.operations.push(op);
        self.generation += 1;
    }

    pub fn remove_operation_type(&mut self, kind: FilterKind) -> bool {
        let before = self.operations.len();
        self.operations.retain(|op| op.kind() != kind);
        if self.operations.len() == before {
            false
        } else {
            self.generation += 1;
            true
        }
    }

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
            result = apply_operation(&result, op, None);
        }

        Some(result)
    }
}

pub(crate) fn apply_operation(
    image: &DecodedImage,
    op: &FilterOperation,
    selection: Option<&PixelRect>,
) -> DecodedImage {
    match op {
        FilterOperation::FlipHorizontal => filter::transform::flip_horizontal(image),
        FilterOperation::FlipVertical => filter::transform::flip_vertical(image),
        FilterOperation::Rotate180 => filter::transform::rotate_180(image),
        FilterOperation::Rotate90CW => filter::transform::rotate_90(image),
        FilterOperation::Rotate90CCW => filter::transform::rotate_270(image),
        FilterOperation::Fill { red, green, blue } => {
            filter::color::fill(image, selection, *red, *green, *blue)
        }
        FilterOperation::Levels { low, high } => {
            filter::brightness::levels(image, selection, *low, *high)
        }
        FilterOperation::Gamma { value } => filter::brightness::gamma(image, selection, *value),
        FilterOperation::BrightnessContrast {
            brightness,
            contrast,
        } => filter::brightness::brightness_contrast(image, selection, *brightness, *contrast),
        FilterOperation::GrayscaleSimple => filter::color::grayscale_simple(image, selection),
        FilterOperation::GrayscaleStrict => filter::color::grayscale_strict(image, selection),
        FilterOperation::Blur => filter::blur::blur(image, selection),
        FilterOperation::BlurStrong => filter::blur::blur_strong(image, selection),
        FilterOperation::Sharpen => filter::sharpen::sharpen(image, selection),
        FilterOperation::SharpenStrong => filter::sharpen::sharpen_strong(image, selection),
        FilterOperation::Mosaic { size } => filter::blur::mosaic(image, selection, *size),
        FilterOperation::GaussianBlur { radius } => {
            filter::blur::gaussian_blur(image, selection, *radius)
        }
        FilterOperation::UnsharpMask { radius } => {
            filter::blur::unsharp_mask(image, selection, *radius)
        }
        FilterOperation::MedianFilter => filter::blur::median_filter(image, selection),
        FilterOperation::InvertColors => filter::color::invert_colors(image, selection),
        FilterOperation::ApplyAlpha => filter::color::apply_alpha(image, selection),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::persistent_filter_image as test_image;

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
        assert_eq!(result.data[0], 155);
    }

    #[test]
    fn multiple_operations_chain() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::InvertColors);
        pf.add_operation(FilterOperation::Levels {
            low: 100,
            high: 200,
        });
        let img = test_image();
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data, [140, 13, 0, 255]);
    }

    #[test]
    fn removing_a_kind_removes_all_its_parameters_and_preserves_other_operations() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Gamma { value: 1.0 });
        pf.add_operation(FilterOperation::Gamma { value: 2.0 });
        pf.add_operation(FilterOperation::InvertColors);
        assert!(pf.has_operation(FilterKind::Gamma));
        assert!(pf.remove_operation_type(FilterKind::Gamma));
        assert!(!pf.has_operation(FilterKind::Gamma));
        assert_eq!(pf.operations().len(), 1);
        assert!(!pf.remove_operation_type(FilterKind::Gamma));
        let result = pf.apply(&test_image()).unwrap();
        assert_eq!(result.data[0], 155);
    }

    #[test]
    fn three_filter_chain_grayscale_invert_levels() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::GrayscaleStrict);
        pf.add_operation(FilterOperation::InvertColors);
        pf.add_operation(FilterOperation::Levels {
            low: 100,
            high: 200,
        });

        let img = crate::test_helpers::solid_image(1, 1, [0, 180, 0, 77]);
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.width, 1);
        assert_eq!(result.height, 1);
        assert_eq!(result.data, [66, 66, 66, 77]);
    }

    #[test]
    fn rotate_90cw_changes_dimensions() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 255, 0, 255, 0, 255],
            width: 2,
            height: 1,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Rotate90CW);

        let result = pf.apply(&img).unwrap();
        assert_eq!(result.width, 1);
        assert_eq!(result.height, 2);
        assert_eq!(result.data, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn rotate_90ccw_changes_dimensions() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 255, 0, 255, 0, 255],
            width: 2,
            height: 1,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Rotate90CCW);

        let result = pf.apply(&img).unwrap();
        assert_eq!(result.width, 1);
        assert_eq!(result.height, 2);
        assert_eq!(result.data, [0, 255, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn rotate_180_preserves_dimensions() {
        let img = DecodedImage {
            data: [10, 20, 30, 40]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 2,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Rotate180);

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (2, 2));
        assert_eq!(
            result.data,
            [40, 0, 0, 255, 30, 0, 0, 255, 20, 0, 0, 255, 10, 0, 0, 255]
        );
    }

    #[test]
    fn flip_horizontal_swaps_pixels() {
        let img = DecodedImage {
            data: [10, 20, 30, 40]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 2,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::FlipHorizontal);

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (2, 2));
        assert_eq!(
            result.data,
            [20, 0, 0, 255, 10, 0, 0, 255, 40, 0, 0, 255, 30, 0, 0, 255]
        );
    }

    #[test]
    fn flip_vertical_swaps_rows() {
        let img = DecodedImage {
            data: [10, 20, 30, 40]
                .into_iter()
                .flat_map(|r| [r, 0, 0, 255])
                .collect(),
            width: 2,
            height: 2,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::FlipVertical);

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (2, 2));
        assert_eq!(
            result.data,
            [30, 0, 0, 255, 40, 0, 0, 255, 10, 0, 0, 255, 20, 0, 0, 255]
        );
    }

    #[test]
    fn gamma_correction_adjusts_brightness() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Gamma { value: 2.0 });

        let img = crate::test_helpers::solid_image(1, 1, [64, 0, 255, 77]);
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data, [128, 0, 255, 77]);
    }

    #[test]
    fn brightness_contrast_adjustment() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::BrightnessContrast {
            brightness: 28,
            contrast: 64,
        });

        let img = crate::test_helpers::solid_image(1, 1, [100, 120, 72, 77]);
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data, [128, 168, 72, 77]);
    }

    #[test]
    fn grayscale_simple_converts() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::GrayscaleSimple);

        let img = crate::test_helpers::solid_image(1, 1, [30, 90, 180, 77]);
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data, [100, 100, 100, 77]);
    }

    #[test]
    fn apply_alpha_composites_on_white() {
        let img = DecodedImage {
            data: vec![255, 0, 0, 128],
            width: 1,
            height: 1,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::ApplyAlpha);

        let result = pf.apply(&img).unwrap();
        assert_eq!(result.data, [255, 127, 127, 255]);
    }

    #[test]
    fn gaussian_blur_runs_without_panic() {
        let mut img = crate::test_helpers::solid_image(7, 7, [0, 0, 0, 77]);
        img.data[96..99].fill(243);
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::GaussianBlur { radius: 1.0 });

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (7, 7));
        assert_eq!(&result.data[96..100], &[16, 16, 16, 77]);
        assert_eq!(&result.data[100..104], &[14, 14, 14, 77]);
    }

    #[test]
    fn unsharp_mask_runs_without_panic() {
        let mut img = crate::test_helpers::solid_image(7, 7, [100, 100, 100, 77]);
        img.data[96..99].fill(127);
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::UnsharpMask { radius: 1.0 });

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (7, 7));
        assert_eq!(&result.data[96..100], &[153, 153, 153, 77]);
        assert_eq!(&result.data[100..104], &[99, 99, 99, 77]);
    }

    #[test]
    fn median_filter_runs_without_panic() {
        let mut img = crate::test_helpers::solid_image(7, 7, [0, 0, 0, 77]);
        img.data[96..99].fill(255);
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::MedianFilter);

        let result = pf.apply(&img).unwrap();
        assert_eq!((result.width, result.height), (7, 7));
        assert_eq!(&result.data[96..100], &[0, 0, 0, 77]);
    }

    #[test]
    fn levels_clamps_range() {
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::Levels {
            low: 100,
            high: 200,
        });

        let result = pf.apply(&test_image()).unwrap();
        assert_eq!(result.data[0], 0);
        assert_eq!(result.data[1], 128);
        assert_eq!(result.data[2], 255);
    }

    #[test]
    fn four_filter_chain_transforms() {
        let img = DecodedImage {
            data: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
            width: 2,
            height: 2,
        };
        let mut pf = PersistentFilter::new();
        pf.toggle_enabled();
        pf.add_operation(FilterOperation::FlipHorizontal);
        pf.add_operation(FilterOperation::FlipVertical);
        pf.add_operation(FilterOperation::Rotate90CW);
        pf.add_operation(FilterOperation::Rotate90CCW);
        let result = pf.apply(&img).unwrap();
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
        assert_eq!(&result.data[0..4], &[10, 11, 12, 255]);
        assert_eq!(&result.data[12..16], &[1, 2, 3, 255]);
    }

    #[test]
    fn toggle_twice_restores_disabled() {
        let mut pf = PersistentFilter::new();
        assert!(!pf.is_enabled());
        pf.toggle_enabled();
        assert!(pf.is_enabled());
        pf.toggle_enabled();
        assert!(!pf.is_enabled());
        pf.add_operation(FilterOperation::InvertColors);
        assert!(pf.apply(&test_image()).is_none());
    }

    #[test]
    fn blur_and_sharpen_variants() {
        let mut img = crate::test_helpers::solid_image(5, 5, [100, 100, 100, 77]);
        img.data[48..51].fill(125);
        for (operation, expected) in [
            (FilterOperation::Blur, 102),
            (FilterOperation::BlurStrong, 101),
            (FilterOperation::Sharpen, 225),
            (FilterOperation::SharpenStrong, 255),
        ] {
            let mut pf = PersistentFilter::new();
            pf.toggle_enabled();
            pf.add_operation(operation);
            let result = pf.apply(&img).unwrap();
            assert_eq!(&result.data[48..52], &[expected, expected, expected, 77]);
        }
    }
}
