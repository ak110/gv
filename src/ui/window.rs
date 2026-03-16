use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

/// ウィンドウクラス登録
pub fn register_window_class(class_name: PCWSTR, wnd_proc: WNDPROC) -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None).context("GetModuleHandleW失敗")?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: wnd_proc,
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH::default(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            anyhow::bail!("RegisterClassExW失敗");
        }
    }
    Ok(())
}

/// ウィンドウ作成
pub fn create_window(class_name: PCWSTR, title: PCWSTR, width: i32, height: i32) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).context("GetModuleHandleW失敗")?;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            title,
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        Ok(hwnd)
    }
}

/// GWLP_USERDATAにポインタを格納
pub fn set_window_data<T>(hwnd: HWND, data: *mut T) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
    }
}

/// GWLP_USERDATAからポインタを取得
pub fn get_window_data<T>(hwnd: HWND) -> Option<&'static mut T> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut T;
        if ptr.is_null() { None } else { Some(&mut *ptr) }
    }
}

/// クライアント領域のサイズを取得
#[allow(dead_code)]
pub fn get_client_size(hwnd: HWND) -> (u32, u32) {
    unsafe {
        let mut rc = std::mem::zeroed();
        let _ = GetClientRect(hwnd, &mut rc);
        ((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32)
    }
}

/// メッセージループ実行
pub fn run_message_loop() -> i32 {
    unsafe {
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam.0 as i32
    }
}
