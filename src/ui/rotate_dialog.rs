//! 角度指定回転
use super::form_dialog::{FieldDef, NumberRule, show_form_dialog};
use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
pub fn show_rotate_dialog(parent: HWND) -> Result<Option<f64>> {
    let fields = [FieldDef {
        label: "回転角度 (度)".into(),
        default: "0".into(),
        rule: NumberRule::Decimal {
            min: None,
            max: None,
        },
    }];
    show_form_dialog(parent, "角度指定回転", &fields)?
        .map(|values| {
            values[0]
                .parse::<f64>()
                .context("回転角度を取得できませんでした")
        })
        .transpose()
}
