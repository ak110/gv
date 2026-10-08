//! 解像度変更
use super::form_dialog::{FieldDef, NumberRule, show_form_dialog};
use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
pub fn show_resize_dialog(
    parent: HWND,
    current_width: u32,
    current_height: u32,
) -> Result<Option<(u32, u32)>> {
    let fields: Vec<_> = [("幅", current_width), ("高さ", current_height)]
        .into_iter()
        .map(|(label, value)| FieldDef {
            label: label.into(),
            default: value.to_string(),
            rule: NumberRule::Integer {
                min: 1,
                max: i64::from(u32::MAX),
            },
        })
        .collect();
    show_form_dialog(parent, "解像度の変更", &fields)?
        .map(|values| {
            Ok((
                values[0]
                    .parse::<u32>()
                    .context("幅を取得できませんでした")?,
                values[1]
                    .parse::<u32>()
                    .context("高さを取得できませんでした")?,
            ))
        })
        .transpose()
}
