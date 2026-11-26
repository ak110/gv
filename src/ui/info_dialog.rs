use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::dialog::{ID_EDIT_BASE, client_size, create_control, show_dialog};

pub fn show_info_dialog(parent: HWND, title: &str, text: &str, font: HFONT) -> Result<()> {
    show_dialog(
        parent,
        title,
        (640, 480),
        |hwnd| {
            let (width, height) = client_size(hwnd)?;
            let edit = create_control(
                hwnd,
                "EDIT",
                &text.replace('\n', "\r\n"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
                WS_EX_CLIENTEDGE,
                ID_EDIT_BASE,
                (0, 0, width, height),
            )?;
            if !font.is_invalid() {
                unsafe {
                    SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }
            Ok(())
        },
        |_| true,
    )
}
