//! 選択・コピーできる情報表示
use anyhow::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HFONT;
pub fn show_info_dialog(parent: HWND, title: &str, text: &str, font: HFONT) -> Result<()> {
    super::dialog::show_dialog(
        parent,
        title,
        super::dialog::DialogContent::Info {
            text: text.into(),
            font,
        },
    )?;
    Ok(())
}
