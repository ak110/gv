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

    /// 表示希望を先に確定し、同期通知を送るWin32操作を所有値で返す。
    pub fn prepare_toggle(
        &mut self,
        hwnd: HWND,
        always_on_top: bool,
        keep_titlebar: bool,
    ) -> impl FnOnce() + 'static + use<> {
        let entering = !self.is_fullscreen;
        if entering {
            unsafe {
                self.saved_style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
                self.saved_ex_style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
                let _ = GetWindowPlacement(hwnd, std::ptr::from_mut(&mut self.saved_placement));
            }
        }
        self.is_fullscreen = entering;
        let saved_style = self.saved_style;
        let saved_ex_style = self.saved_ex_style;
        let saved_placement = self.saved_placement;
        move || unsafe {
            if entering {
                let new_style = if keep_titlebar {
                    WINDOW_STYLE(saved_style.0 | WS_VISIBLE.0)
                } else {
                    WINDOW_STYLE(
                        (saved_style.0 & !WS_OVERLAPPEDWINDOW.0) | WS_POPUP.0 | WS_VISIBLE.0,
                    )
                };
                SetWindowLongPtrW(hwnd, GWL_STYLE, new_style.0 as isize);
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
            } else {
                SetWindowLongPtrW(hwnd, GWL_STYLE, saved_style.0 as isize);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, saved_ex_style.0 as isize);
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
                let _ = SetWindowPlacement(hwnd, std::ptr::from_ref(&saved_placement));
            }
        }
    }
}
