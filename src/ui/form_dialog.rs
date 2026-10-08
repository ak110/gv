//! 数値フォームの項目定義と入力検証

use anyhow::Result;
use windows::Win32::Foundation::HWND;

#[derive(Clone, Copy, Debug)]
pub enum NumberRule {
    Integer { min: i64, max: i64 },
    Decimal { min: Option<f64>, max: Option<f64> },
}

impl NumberRule {
    pub fn accepts(self, text: &str) -> bool {
        match self {
            Self::Integer { min, max } => text
                .trim()
                .parse::<i64>()
                .is_ok_and(|value| (min..=max).contains(&value)),
            Self::Decimal { min, max } => text.trim().parse::<f64>().is_ok_and(|value| {
                value.is_finite()
                    && min.is_none_or(|min| value >= min)
                    && max.is_none_or(|max| value <= max)
            }),
        }
    }

    pub fn range_label(self) -> String {
        match self {
            Self::Integer { min, max } => format!("{min}〜{max}、整数"),
            Self::Decimal {
                min: Some(min),
                max: Some(max),
            } => format!("{min}〜{max}"),
            Self::Decimal {
                min: None,
                max: None,
            } => "有限の数値".into(),
            Self::Decimal {
                min: Some(min),
                max: None,
            } => format!("{min}以上"),
            Self::Decimal {
                min: None,
                max: Some(max),
            } => format!("{max}以下"),
        }
    }
}

#[derive(Clone)]
pub struct FieldDef {
    pub label: String,
    pub default: String,
    pub rule: NumberRule,
}

impl FieldDef {
    pub fn display_label(&self) -> String {
        format!("{} ({})", self.label, self.rule.range_label())
    }
}

pub fn show_form_dialog(
    parent: HWND,
    title: &str,
    fields: &[FieldDef],
) -> Result<Option<Vec<String>>> {
    if fields.is_empty() {
        return Ok(None);
    }
    super::dialog::show_dialog(
        parent,
        title,
        super::dialog::DialogContent::Form(fields.to_vec()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_inputs_reject_overflow_non_numbers_and_non_finite_values() {
        for (rule, valid, invalid) in [
            (
                NumberRule::Integer { min: 1, max: 10 },
                vec!["1", "10", " 5 "],
                vec!["0", "11", "1.5", "abc", "", "9223372036854775808"],
            ),
            (
                NumberRule::Decimal {
                    min: Some(0.1),
                    max: Some(10.0),
                },
                vec!["0.1", "10", "1e0"],
                vec!["0", "10.1", "NaN", "inf", "-inf", "abc", ""],
            ),
            (
                NumberRule::Decimal {
                    min: None,
                    max: None,
                },
                vec!["-360", "0", "360.5"],
                vec!["NaN", "inf", "-inf", "1e999", "abc", ""],
            ),
        ] {
            for value in valid {
                assert!(rule.accepts(value), "{rule:?}: {value}");
            }
            for value in invalid {
                assert!(!rule.accepts(value), "{rule:?}: {value}");
            }
        }
    }
}
