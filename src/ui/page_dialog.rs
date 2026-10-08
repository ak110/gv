//! ページ指定移動
use super::form_dialog::{FieldDef, NumberRule, show_form_dialog};
use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
pub fn show_page_dialog(parent: HWND, current: usize, total: usize) -> Result<Option<usize>> {
    let fields = [FieldDef {
        label: "ページ番号".into(),
        default: current.to_string(),
        rule: NumberRule::Integer {
            min: 1,
            max: i64::try_from(total).context("ページ数が大き過ぎます")?,
        },
    }];
    show_form_dialog(parent, "ページ指定移動", &fields)?
        .map(|values| {
            values[0]
                .parse::<usize>()
                .context("ページ番号を取得できませんでした")
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialog::{
        assert_dialog_unconfirmed, assert_test_input_selected, submit_test_value, with_test_driver,
    };

    #[test]
    fn invalid_page_input_keeps_the_same_window_until_corrected() {
        let result = with_test_driver(
            |hwnd| {
                for input in ["0", "4", "abc"] {
                    submit_test_value(hwnd, 0, input);
                    assert_dialog_unconfirmed(hwnd);
                    assert_test_input_selected(hwnd, 0, input.len() as u32);
                }
                submit_test_value(hwnd, 0, "3");
            },
            || show_page_dialog(HWND::default(), 2, 3),
        );
        assert_eq!(result.unwrap(), Some(3));
    }
}
