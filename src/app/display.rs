//! 表示モード・拡大縮小・ウィンドウ表示の操作

use super::AppWindow;
use crate::render::layout::DisplayMode;
use crate::ui::window;
use windows::Win32::UI::WindowsAndMessaging::*;

impl AppWindow {
    /// 現在の画像サイズを返す (zoom操作用)
    pub(super) fn current_image_size(&self) -> Option<(u32, u32)> {
        self.document
            .current_image()
            .map(|img| (img.width, img.height))
    }

    /// クライアント領域のサイズを返す
    pub(super) fn client_size(&self) -> (f32, f32) {
        let (w, h) = window::get_client_size(self.hwnd);
        (w as f32, h as f32)
    }

    /// 常に手前に表示をトグル
    pub(super) fn toggle_always_on_top(&mut self) {
        self.always_on_top = !self.always_on_top;
        // フルスクリーン中は復帰時に反映されるので今は何もしない
        if !self.fullscreen.is_fullscreen() {
            let z_order = if self.always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };
            let hwnd = self.hwnd;
            self.defer_ui(move || unsafe {
                let _ = SetWindowPos(hwnd, Some(z_order), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            });
        }
    }

    /// フルスクリーンをトグル
    pub(super) fn toggle_fullscreen(&mut self) {
        let entering = !self.fullscreen.is_fullscreen();

        let change = self.fullscreen.prepare_toggle(
            self.hwnd,
            self.always_on_top,
            self.keep_titlebar_in_fullscreen,
        );
        self.defer_ui(change);

        let hwnd = self.hwnd;
        if entering {
            // フルスクリーン開始: メニュー・パネルを非表示 (フラグは保持)
            self.defer_ui(move || unsafe {
                let _ = SetMenu(hwnd, None);
            });
        } else {
            // フルスクリーン解除: カーソル復帰、メニュー・パネルを復元
            self.cursor_hider.force_show(self.hwnd);
            if self.menu_visible {
                let menu = self.menu;
                self.defer_ui(move || unsafe {
                    let _ = SetMenu(hwnd, Some(menu));
                });
            }
        }
        self.file_list_panel
            .sync_visibility(self.fullscreen.is_fullscreen());
        let panel = self.file_list_panel.clone();
        self.defer_ui(move || panel.apply_visibility());
        self.sync_file_list_panel();
        // SetWindowPosが送るWM_SIZEの後に、確定した実表示で画像領域を再配置する。
        self.defer_call(
            move || window::get_client_size(hwnd),
            |app, (width, height)| app.on_size(width, height),
        );
    }

    /// 最大化トグル (左ダブルクリック)
    pub(super) fn toggle_maximize(&self) {
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            let _ = GetWindowPlacement(hwnd, std::ptr::from_mut(&mut placement));
            if placement.showCmd == SW_MAXIMIZE.0 as u32 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
        });
    }

    pub(super) fn action_toggle_menu_bar(&mut self) {
        // フルスクリーン中はメニューを常に非表示にしているため、
        // 表示状態だけが切り替わらないよう無視する
        if !self.fullscreen.is_fullscreen() {
            self.menu_visible = !self.menu_visible;
            let hwnd = self.hwnd;
            let menu = self.menu_visible.then_some(self.menu);
            self.defer_ui(move || unsafe {
                let _ = SetMenu(hwnd, menu);
            });
        }
    }

    pub(super) fn action_toggle_cursor_hide(&mut self) {
        self.cursor_hider.toggle_enabled(self.hwnd);
    }

    pub(super) fn action_toggle_maximize(&mut self) {
        if !self.fullscreen.is_fullscreen() {
            self.toggle_maximize();
        }
    }

    pub(super) fn action_minimize(&mut self) {
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        });
    }

    pub(super) fn action_cycle_alpha_background(&mut self) {
        self.renderer.cycle_alpha_background();
        self.invalidate();
    }

    pub(super) fn action_toggle_margin(&mut self) {
        self.renderer.layout_mut().toggle_margin();
        self.invalidate();
    }

    pub(super) fn action_zoom_reset(&mut self) {
        self.renderer.layout_mut().zoom_reset();
        self.invalidate();
    }

    pub(super) fn action_zoom_out(&mut self) {
        if let Some((iw, ih)) = self.current_image_size() {
            let (ww, wh) = self.client_size();
            self.renderer.layout_mut().zoom_out(iw, ih, ww, wh);
            self.invalidate();
        }
    }

    pub(super) fn action_zoom_in(&mut self) {
        if let Some((iw, ih)) = self.current_image_size() {
            let (ww, wh) = self.client_size();
            self.renderer.layout_mut().zoom_in(iw, ih, ww, wh);
            self.invalidate();
        }
    }

    pub(super) fn action_display_auto_fit(&mut self) {
        self.renderer.layout_mut().mode = DisplayMode::AutoFit;
        self.invalidate();
    }

    pub(super) fn action_display_auto_shrink(&mut self) {
        self.renderer.layout_mut().mode = DisplayMode::AutoShrink;
        self.invalidate();
    }
}
