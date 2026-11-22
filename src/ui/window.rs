use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

pub fn register_window_class_with_icon(
    class_name: PCWSTR,
    wnd_proc: WNDPROC,
    icon: Option<HICON>,
) -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None).context("GetModuleHandleW失敗")?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: wnd_proc,
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH::default(),
            hIcon: icon.unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let atom = RegisterClassExW(std::ptr::from_ref(&wc));
        if atom == 0
            && windows::Win32::Foundation::GetLastError()
                != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
        {
            anyhow::bail!("RegisterClassExW失敗");
        }
    }
    Ok(())
}

pub fn create_window(class_name: PCWSTR, title: PCWSTR, width: i32, height: i32) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).context("GetModuleHandleW失敗")?;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            title,
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
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

pub fn set_window_data<T>(hwnd: HWND, data: *mut T) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
    }
}

pub fn get_client_size(hwnd: HWND) -> (u32, u32) {
    unsafe {
        let mut rc = std::mem::zeroed();
        let _ = GetClientRect(hwnd, std::ptr::from_mut(&mut rc));
        ((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32)
    }
}

pub fn run_message_loop() -> i32 {
    unsafe {
        let mut msg = std::mem::zeroed();
        while GetMessageW(std::ptr::from_mut(&mut msg), None, 0, 0).as_bool() {
            let _ = TranslateMessage(std::ptr::from_ref(&msg));
            DispatchMessageW(std::ptr::from_ref(&msg));
        }
        msg.wParam.0 as i32
    }
}
