use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MonitorFromWindow};
use windows::Win32::UI::WindowsAndMessaging::*;

/// フルスクリーン状態管理 (ボーダーレス最大化)
pub struct FullscreenState {
    is_fullscreen: bool,
    saved_style: WINDOW_STYLE,
    saved_ex_style: WINDOW_EX_STYLE,
    saved_placement: WINDOWPLACEMENT,
}

impl FullscreenState {
    pub fn new() -> Self {
        Self {
            is_fullscreen: false,
            saved_style: WINDOW_STYLE::default(),
            saved_ex_style: WINDOW_EX_STYLE::default(),
            saved_placement: WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            },
        }
    }

    pub fn is_fullscreen(&self) -> bool {
        self.is_fullscreen
    }

    /// フルスクリーンをトグル
    pub fn toggle(&mut self, hwnd: HWND, always_on_top: bool) {
        if self.is_fullscreen {
            self.exit_fullscreen(hwnd, always_on_top);
        } else {
            self.enter_fullscreen(hwnd);
        }
    }

    fn enter_fullscreen(&mut self, hwnd: HWND) {
        unsafe {
            // 現在のスタイル・配置を保存
            self.saved_style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
            self.saved_ex_style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
            self.saved_placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            let _ = GetWindowPlacement(hwnd, std::ptr::from_mut(&mut self.saved_placement));

            // ウィンドウ枠を外してポップアップスタイルに変更
            let new_style = WINDOW_STYLE(
                (self.saved_style.0 & !WS_OVERLAPPEDWINDOW.0) | WS_POPUP.0 | WS_VISIBLE.0,
            );
            SetWindowLongPtrW(hwnd, GWL_STYLE, new_style.0 as isize);

            // モニター領域を取得して全画面化
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, std::ptr::from_mut(&mut mi)).as_bool() {
                let rc = mi.rcMonitor;
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                );
            }

            self.is_fullscreen = true;
        }
    }

    fn exit_fullscreen(&mut self, hwnd: HWND, always_on_top: bool) {
        unsafe {
            // スタイルを復元
            SetWindowLongPtrW(hwnd, GWL_STYLE, self.saved_style.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, self.saved_ex_style.0 as isize);

            // Z-orderとフレーム再計算
            let z_order = if always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };
            let _ = SetWindowPos(
                hwnd,
                Some(z_order),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
            );

            // ウィンドウ位置・サイズを復元
            let _ = SetWindowPlacement(hwnd, std::ptr::from_ref(&self.saved_placement));

            self.is_fullscreen = false;
        }
    }
}
