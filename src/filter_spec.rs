//! フィルターの操作対応とパラメーター仕様

use crate::action::Action;
use crate::persistent_filter::FilterOperation;
use crate::ui::form_dialog::{FieldDef, NumberRule};
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    FlipHorizontal,
    FlipVertical,
    Rotate180,
    Rotate90CW,
    Rotate90CCW,
    Fill,
    Levels,
    Gamma,
    BrightnessContrast,
    GrayscaleSimple,
    GrayscaleStrict,
    Blur,
    BlurStrong,
    Sharpen,
    SharpenStrong,
    Mosaic,
    GaussianBlur,
    UnsharpMask,
    MedianFilter,
    InvertColors,
    ApplyAlpha,
}

pub struct ParameterSpec {
    pub label: &'static str,
    pub default: &'static str,
    pub rule: NumberRule,
}

pub struct FilterSpec {
    pub kind: FilterKind,
    pub title: &'static str,
    pub action: Action,
    pub persistent_action: Option<Action>,
    pub parameters: &'static [ParameterSpec],
}

use Action as A;
use FilterKind as K;
use NumberRule::{Decimal, Integer};

pub static FILTER_SPECS: &[FilterSpec] = &[
    FilterSpec {
        kind: K::FlipHorizontal,
        title: "左右反転",
        action: A::FlipHorizontal,
        persistent_action: Some(A::PFilterFlipH),
        parameters: &[],
    },
    FilterSpec {
        kind: K::FlipVertical,
        title: "上下反転",
        action: A::FlipVertical,
        persistent_action: Some(A::PFilterFlipV),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Rotate180,
        title: "180度回転",
        action: A::Rotate180,
        persistent_action: Some(A::PFilterRotate180),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Rotate90CW,
        title: "右90度回転",
        action: A::Rotate90CW,
        persistent_action: Some(A::PFilterRotate90CW),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Rotate90CCW,
        title: "左90度回転",
        action: A::Rotate90CCW,
        persistent_action: Some(A::PFilterRotate90CCW),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Fill,
        title: "塗り潰す",
        action: A::Fill,
        persistent_action: None,
        parameters: &[
            ParameterSpec {
                label: "赤",
                default: "255",
                rule: Integer { min: 0, max: 255 },
            },
            ParameterSpec {
                label: "緑",
                default: "255",
                rule: Integer { min: 0, max: 255 },
            },
            ParameterSpec {
                label: "青",
                default: "255",
                rule: Integer { min: 0, max: 255 },
            },
        ],
    },
    FilterSpec {
        kind: K::Levels,
        title: "レベル補正",
        action: A::Levels,
        persistent_action: Some(A::PFilterLevels),
        parameters: &[
            ParameterSpec {
                label: "下限",
                default: "0",
                rule: Integer { min: 0, max: 255 },
            },
            ParameterSpec {
                label: "上限",
                default: "255",
                rule: Integer { min: 0, max: 255 },
            },
        ],
    },
    FilterSpec {
        kind: K::Gamma,
        title: "ガンマ補正",
        action: A::Gamma,
        persistent_action: Some(A::PFilterGamma),
        parameters: &[ParameterSpec {
            label: "ガンマ値",
            default: "1.0",
            rule: Decimal {
                min: Some(0.1),
                max: Some(10.0),
            },
        }],
    },
    FilterSpec {
        kind: K::BrightnessContrast,
        title: "明るさとコントラスト",
        action: A::BrightnessContrast,
        persistent_action: Some(A::PFilterBrightnessContrast),
        parameters: &[
            ParameterSpec {
                label: "明るさ",
                default: "0",
                rule: Integer {
                    min: -128,
                    max: 128,
                },
            },
            ParameterSpec {
                label: "コントラスト",
                default: "0",
                rule: Integer {
                    min: -128,
                    max: 128,
                },
            },
        ],
    },
    FilterSpec {
        kind: K::GrayscaleSimple,
        title: "簡易グレースケール",
        action: A::GrayscaleSimple,
        persistent_action: Some(A::PFilterGrayscaleSimple),
        parameters: &[],
    },
    FilterSpec {
        kind: K::GrayscaleStrict,
        title: "厳密グレースケール",
        action: A::GrayscaleStrict,
        persistent_action: Some(A::PFilterGrayscaleStrict),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Blur,
        title: "ぼかす",
        action: A::Blur,
        persistent_action: Some(A::PFilterBlur),
        parameters: &[],
    },
    FilterSpec {
        kind: K::BlurStrong,
        title: "強くぼかす",
        action: A::BlurStrong,
        persistent_action: Some(A::PFilterBlurStrong),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Sharpen,
        title: "シャープ",
        action: A::Sharpen,
        persistent_action: Some(A::PFilterSharpen),
        parameters: &[],
    },
    FilterSpec {
        kind: K::SharpenStrong,
        title: "強くシャープ",
        action: A::SharpenStrong,
        persistent_action: Some(A::PFilterSharpenStrong),
        parameters: &[],
    },
    FilterSpec {
        kind: K::Mosaic,
        title: "モザイク",
        action: A::Mosaic,
        persistent_action: None,
        parameters: &[ParameterSpec {
            label: "ブロックサイズ",
            default: "10",
            rule: Integer {
                min: 1,
                max: u32::MAX as i64,
            },
        }],
    },
    FilterSpec {
        kind: K::GaussianBlur,
        title: "ガウスぼかし",
        action: A::GaussianBlur,
        persistent_action: Some(A::PFilterGaussianBlur),
        parameters: &[ParameterSpec {
            label: "半径",
            default: "2.0",
            rule: Decimal {
                min: Some(0.1),
                max: Some(10.0),
            },
        }],
    },
    FilterSpec {
        kind: K::UnsharpMask,
        title: "アンシャープマスク",
        action: A::UnsharpMask,
        persistent_action: Some(A::PFilterUnsharpMask),
        parameters: &[ParameterSpec {
            label: "半径",
            default: "2.0",
            rule: Decimal {
                min: Some(0.1),
                max: Some(10.0),
            },
        }],
    },
    FilterSpec {
        kind: K::MedianFilter,
        title: "メディアンフィルター",
        action: A::MedianFilter,
        persistent_action: Some(A::PFilterMedianFilter),
        parameters: &[],
    },
    FilterSpec {
        kind: K::InvertColors,
        title: "色反転",
        action: A::InvertColors,
        persistent_action: Some(A::PFilterInvertColors),
        parameters: &[],
    },
    FilterSpec {
        kind: K::ApplyAlpha,
        title: "α合成",
        action: A::ApplyAlpha,
        persistent_action: Some(A::PFilterApplyAlpha),
        parameters: &[],
    },
];

pub fn for_action(action: Action) -> Option<(&'static FilterSpec, bool)> {
    FILTER_SPECS.iter().find_map(|spec| {
        if spec.action == action {
            Some((spec, false))
        } else if spec.persistent_action == Some(action) {
            Some((spec, true))
        } else {
            None
        }
    })
}

impl FilterSpec {
    pub fn fields(&self) -> Vec<FieldDef> {
        self.parameters
            .iter()
            .map(|parameter| FieldDef {
                label: parameter.label.into(),
                default: parameter.default.into(),
                rule: parameter.rule,
            })
            .collect()
    }

    pub fn operation(&self, values: &[String]) -> Result<FilterOperation> {
        if values.len() != self.parameters.len() {
            bail!("入力欄の数が一致しません");
        }
        for (parameter, value) in self.parameters.iter().zip(values) {
            if !parameter.rule.accepts(value) {
                bail!(
                    "{} ({})に従って入力し直してください",
                    parameter.label,
                    parameter.rule.range_label()
                );
            }
        }
        use FilterOperation as O;
        Ok(match self.kind {
            K::FlipHorizontal => O::FlipHorizontal,
            K::FlipVertical => O::FlipVertical,
            K::Rotate180 => O::Rotate180,
            K::Rotate90CW => O::Rotate90CW,
            K::Rotate90CCW => O::Rotate90CCW,
            K::Fill => O::Fill {
                red: values[0].parse()?,
                green: values[1].parse()?,
                blue: values[2].parse()?,
            },
            K::Levels => O::Levels {
                low: values[0].parse()?,
                high: values[1].parse()?,
            },
            K::Gamma => O::Gamma {
                value: values[0].parse()?,
            },
            K::BrightnessContrast => O::BrightnessContrast {
                brightness: values[0].parse()?,
                contrast: values[1].parse()?,
            },
            K::GrayscaleSimple => O::GrayscaleSimple,
            K::GrayscaleStrict => O::GrayscaleStrict,
            K::Blur => O::Blur,
            K::BlurStrong => O::BlurStrong,
            K::Sharpen => O::Sharpen,
            K::SharpenStrong => O::SharpenStrong,
            K::Mosaic => O::Mosaic {
                size: values[0].parse()?,
            },
            K::GaussianBlur => O::GaussianBlur {
                radius: values[0].parse()?,
            },
            K::UnsharpMask => O::UnsharpMask {
                radius: values[0].parse()?,
            },
            K::MedianFilter => O::MedianFilter,
            K::InvertColors => O::InvertColors,
            K::ApplyAlpha => O::ApplyAlpha,
        })
    }
}

impl FilterOperation {
    pub fn kind(&self) -> FilterKind {
        use FilterOperation as O;
        match self {
            O::FlipHorizontal => K::FlipHorizontal,
            O::FlipVertical => K::FlipVertical,
            O::Rotate180 => K::Rotate180,
            O::Rotate90CW => K::Rotate90CW,
            O::Rotate90CCW => K::Rotate90CCW,
            O::Fill { .. } => K::Fill,
            O::Levels { .. } => K::Levels,
            O::Gamma { .. } => K::Gamma,
            O::BrightnessContrast { .. } => K::BrightnessContrast,
            O::GrayscaleSimple => K::GrayscaleSimple,
            O::GrayscaleStrict => K::GrayscaleStrict,
            O::Blur => K::Blur,
            O::BlurStrong => K::BlurStrong,
            O::Sharpen => K::Sharpen,
            O::SharpenStrong => K::SharpenStrong,
            O::Mosaic { .. } => K::Mosaic,
            O::GaussianBlur { .. } => K::GaussianBlur,
            O::UnsharpMask { .. } => K::UnsharpMask,
            O::MedianFilter => K::MedianFilter,
            O::InvertColors => K::InvertColors,
            O::ApplyAlpha => K::ApplyAlpha,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_parameter_dialogs_validate_the_displayed_bounds() {
        for spec in FILTER_SPECS
            .iter()
            .filter(|spec| !spec.parameters.is_empty())
        {
            let defaults: Vec<String> = spec.parameters.iter().map(|p| p.default.into()).collect();
            assert_eq!(spec.operation(&defaults).unwrap().kind(), spec.kind);
            let fields = spec.fields();
            for (index, parameter) in spec.parameters.iter().enumerate() {
                let (bounds, rejected) = match parameter.rule {
                    Integer { min, max } => (
                        vec![min.to_string(), max.to_string()],
                        vec![
                            (min - 1).to_string(),
                            (max + 1).to_string(),
                            "1.5".into(),
                            "abc".into(),
                            String::new(),
                        ],
                    ),
                    Decimal {
                        min: Some(min),
                        max: Some(max),
                    } => (
                        vec![min.to_string(), max.to_string()],
                        vec![
                            (min - 0.01).to_string(),
                            (max + 0.01).to_string(),
                            "NaN".into(),
                            "inf".into(),
                            "abc".into(),
                            String::new(),
                        ],
                    ),
                    Decimal { .. } => {
                        panic!("フィルターのパラメーターには上下限が必要です")
                    }
                };
                for input in bounds {
                    assert!(fields[index].rule.accepts(&input));
                    let mut values = defaults.clone();
                    values[index] = input;
                    assert!(
                        spec.operation(&values).is_ok(),
                        "{:?}: {values:?}",
                        spec.kind
                    );
                }
                for input in rejected {
                    assert!(!fields[index].rule.accepts(&input));
                    let mut values = defaults.clone();
                    values[index] = input;
                    assert!(
                        spec.operation(&values).is_err(),
                        "{:?}: {values:?}",
                        spec.kind
                    );
                }
            }
        }
    }
}
