//! フィルターパラメーター入力
pub use super::form_dialog::FieldDef;
use anyhow::Result;
use windows::Win32::Foundation::HWND;
pub fn show_filter_dialog(
    parent: HWND,
    title: &str,
    fields: &[FieldDef],
) -> Result<Option<Vec<String>>> {
    super::form_dialog::show_form_dialog(parent, title, fields)
}
