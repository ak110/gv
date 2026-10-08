//! マウス入力と矩形選択

use super::AppWindow;
use crate::selection::{HandleKind, HitTestResult};
use crate::ui::key_config::Modifiers;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

const VK_CONTROL: i32 = 0x11;
const VK_SHIFT: i32 = 0x10;
const VK_MENU: i32 = 0x12;

impl AppWindow {
    /// 現在の修飾キー状態を取得
    pub(super) fn current_modifiers() -> Modifiers {
        unsafe {
            Modifiers {
                ctrl: GetKeyState(VK_CONTROL) < 0,
                shift: GetKeyState(VK_SHIFT) < 0,
                alt: GetKeyState(VK_MENU) < 0,
            }
        }
    }

    /// マウス左ボタン押下: 選択ドラッグ開始
    pub(super) fn on_lbutton_down(&mut self, lparam: LPARAM) {
        self.begin_user_operation();
        let Some(draw_rect) = self.renderer.last_draw_rect().copied() else {
            return;
        };
        let Some(img) = self.document.current_image() else {
            return;
        };

        let sx = (lparam.0 & 0xFFFF) as i16 as f32;
        let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

        self.selection
            .on_mouse_down(sx, sy, &draw_rect, img.width, img.height);

        if self.selection.is_dragging() {
            // マウスキャプチャ (ウィンドウ外でもドラッグイベントを受け取る)
            unsafe {
                windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
            }
            self.invalidate();
            self.update_title();
        }
    }

    /// マウス移動: ドラッグ中の矩形更新
    pub(super) fn on_mouse_move(&mut self, lparam: LPARAM) {
        if !self.selection.is_dragging() {
            return;
        }
        let Some(draw_rect) = self.renderer.last_draw_rect().copied() else {
            return;
        };
        let Some(img) = self.document.current_image() else {
            return;
        };

        let sx = (lparam.0 & 0xFFFF) as i16 as f32;
        let sy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

        self.selection
            .on_mouse_move(sx, sy, &draw_rect, img.width, img.height);
        self.invalidate();
        self.update_title();
    }

    /// マウス左ボタンリリース: ドラッグ終了
    pub(super) fn on_lbutton_up(&mut self) {
        if !self.selection.is_dragging() {
            return;
        }
        let Some(img) = self.document.current_image() else {
            return;
        };

        unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture().unwrap_or_default();
        }
        self.selection.on_mouse_up(img.width, img.height);
        self.invalidate();
        self.update_title();
    }

    /// WM_SETCURSOR: 選択ハンドル上でカーソルを変更
    /// trueを返した場合はDefWindowProcを呼ばない
    pub(super) fn on_set_cursor(&self) -> bool {
        if !self.selection.is_selected() {
            return false;
        }
        let Some(draw_rect) = self.renderer.last_draw_rect() else {
            return false;
        };
        let Some(img) = self.document.current_image() else {
            return false;
        };

        // 現在のマウス位置を取得
        let mut pt = windows::Win32::Foundation::POINT::default();
        unsafe {
            let _ =
                windows::Win32::UI::WindowsAndMessaging::GetCursorPos(std::ptr::from_mut(&mut pt));
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(
                self.hwnd,
                std::ptr::from_mut(&mut pt),
            );
        }
        let sx = pt.x as f32;
        let sy = pt.y as f32;

        let hit = self
            .selection
            .hit_test_at(sx, sy, draw_rect, img.width, img.height);
        let cursor_id = match hit {
            HitTestResult::Handle(HandleKind::TopLeft | HandleKind::BottomRight) => {
                Some(IDC_SIZENWSE)
            }
            HitTestResult::Handle(HandleKind::TopRight | HandleKind::BottomLeft) => {
                Some(IDC_SIZENESW)
            }
            HitTestResult::Handle(HandleKind::Top | HandleKind::Bottom) => Some(IDC_SIZENS),
            HitTestResult::Handle(HandleKind::Left | HandleKind::Right) => Some(IDC_SIZEWE),
            HitTestResult::Inside => Some(IDC_SIZEALL),
            _ => None,
        };

        if let Some(id) = cursor_id {
            unsafe {
                let _ = SetCursor(LoadCursorW(None, id).ok());
            }
            return true;
        }

        false
    }

    pub(super) fn action_deselect_selection(&mut self) {
        self.selection.deselect();
        self.invalidate();
        self.update_title();
    }
}
