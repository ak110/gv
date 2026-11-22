use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, ShowCursor};

pub const TIMER_ID_CURSOR_HIDE: usize = 1;
const CURSOR_HIDE_DELAY_MS: u32 = 3000;

pub struct CursorHider {
    hidden: bool,
    enabled: bool,
}

impl CursorHider {
    pub fn new() -> Self {
        Self {
            hidden: false,
            enabled: true,
        }
    }

    pub fn on_mouse_move(&mut self, hwnd: HWND) {
        if !self.enabled {
            return;
        }
        self.show_cursor();
        unsafe {
            let _ = SetTimer(Some(hwnd), TIMER_ID_CURSOR_HIDE, CURSOR_HIDE_DELAY_MS, None);
        }
    }

    pub fn on_timer(&mut self, hwnd: HWND) {
        if !self.enabled {
            return;
        }
        self.hide_cursor();
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_ID_CURSOR_HIDE);
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle_enabled(&mut self, hwnd: HWND) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.show_cursor();
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_ID_CURSOR_HIDE);
            }
        }
    }

    pub fn force_show(&mut self, hwnd: HWND) {
        self.show_cursor();
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_ID_CURSOR_HIDE);
        }
    }

    fn show_cursor(&mut self) {
        if self.hidden {
            unsafe {
                ShowCursor(true);
            }
            self.hidden = false;
        }
    }

    fn hide_cursor(&mut self) {
        if !self.hidden {
            unsafe {
                ShowCursor(false);
            }
            self.hidden = true;
        }
    }
}
