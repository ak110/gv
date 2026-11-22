use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::AppWindow;

pub(super) const TIMER_ID_SLIDESHOW: usize = 2;

impl AppWindow {
    pub(crate) fn toggle_slideshow(&mut self) {
        if self.slideshow_active {
            self.stop_slideshow();
        } else {
            self.start_slideshow();
        }
    }

    pub(crate) fn start_slideshow(&mut self) {
        if self.document.file_list().len() < 2 {
            return;
        }
        self.slideshow_active = true;
        unsafe {
            let _ = SetTimer(
                Some(self.hwnd),
                TIMER_ID_SLIDESHOW,
                self.slideshow_interval_ms,
                None,
            );
        }
    }

    pub(crate) fn stop_slideshow(&mut self) {
        if !self.slideshow_active {
            return;
        }
        self.slideshow_active = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ID_SLIDESHOW);
        }
    }

    pub(crate) fn on_slideshow_timer(&mut self) {
        if !self.slideshow_active {
            return;
        }
        let at_end = self
            .document
            .file_list()
            .current_index()
            .is_some_and(|idx| idx + 1 >= self.document.file_list().len());

        if at_end {
            if self.slideshow_repeat {
                self.navigate_with_guard(crate::document::Document::navigate_first);
            } else {
                self.stop_slideshow();
            }
            return;
        }
        self.navigate_with_guard(|document| document.navigate_relative(1));
    }

    pub(crate) fn adjust_slideshow_interval(&mut self, delta_ms: i32) {
        let new_val =
            (i64::from(self.slideshow_interval_ms) + i64::from(delta_ms)).clamp(500, 30_000) as u32;
        self.slideshow_interval_ms = new_val;
        if self.slideshow_active {
            unsafe {
                let _ = SetTimer(
                    Some(self.hwnd),
                    TIMER_ID_SLIDESHOW,
                    self.slideshow_interval_ms,
                    None,
                );
            }
        }
    }
}
