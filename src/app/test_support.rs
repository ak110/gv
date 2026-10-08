//! 非表示ウィンドウを使うテスト支援

use super::*;
use std::path::Path;

/// 表示しないAppWindow。破棄時にウィンドウを閉じる (WM_DESTROYの後始末を通す)
pub(crate) struct TestApp(Box<WindowState>);

impl TestApp {
    pub(crate) fn new() -> Self {
        Self(AppWindow::create_hidden_for_test())
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.0.hwnd()
    }

    /// 操作の借用を終了してから、再入を伴うUI処理を実行する
    pub(crate) fn with_app<R>(&self, f: impl FnOnce(&mut AppWindow) -> R) -> R {
        self.0.with_app(f)
    }

    pub(crate) fn borrow(&self) -> std::cell::Ref<'_, AppWindow> {
        self.0.borrow()
    }

    /// ウィンドウを閉じる。WM_DESTROYでポインタが外れた後にAppWindowを解放する
    pub(crate) fn destroy(self) {
        drop(self);
    }

    /// ウィンドウを閉じ、本体と同じくAppWindowを解放せずに残す
    ///
    /// 本体はメッセージループの後に`process::exit`で終わり、AppWindowを解放しない。
    /// PDFを描画した後に、ウィンドウ破棄済みのD2DRendererを解放すると、
    /// テストプロセスの終了時に終了コードが2170になる。測定では本体と同じ終了経路にそろえる。
    pub(crate) fn close_without_release(self) {
        let this = std::mem::ManuallyDrop::new(self);
        unsafe {
            let _ = DestroyWindow(this.hwnd());
        }
    }

    pub(crate) fn title(&self) -> String {
        let mut buf = [0u16; 1024];
        let len = unsafe { GetWindowTextW(self.hwnd(), &mut buf) };
        String::from_utf16_lossy(&buf[..len as usize])
    }

    /// 描画を1回処理し、その処理が再描画を要求したかを返す
    pub(crate) fn paint_and_check_redraw(&self) -> bool {
        let before = self.borrow().redraw_request_count.get();
        self.with_app(AppWindow::on_paint);
        self.borrow().redraw_request_count.get() != before
    }

    /// 画像ファイルを単独で開き、表示まで処理する
    pub(crate) fn open_image_file(&self, path: &Path) {
        self.with_app(|app| app.document.open_single(path).unwrap());
        self.with_app(AppWindow::process_document_events);
        assert!(self.borrow().document.current_image().is_some());
    }

    /// 画像座標の2点をドラッグして選択範囲を設定する (描画済みであること)
    pub(crate) fn drag_select(&self, from: (i32, i32), to: (i32, i32)) {
        let (rect, w, h) = {
            let app = self.borrow();
            let rect = *app.renderer.last_draw_rect().expect("drawn");
            let img = app.document.current_image().expect("image");
            (rect, img.width, img.height)
        };
        // 画素の中心を指す (画素の端では整数化で隣の画素や画像外へずれるため)
        let lparam = |p: (i32, i32)| {
            let sx = rect.x + (p.0 as f32 + 0.5) / w as f32 * rect.width;
            let sy = rect.y + (p.1 as f32 + 0.5) / h as f32 * rect.height;
            LPARAM(((sy as isize) << 16) | (sx as isize & 0xFFFF))
        };
        self.with_app(|app| app.on_lbutton_down(lparam(from)));
        self.with_app(|app| app.on_mouse_move(lparam(to)));
        self.with_app(AppWindow::on_lbutton_up);
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd());
        }
    }
}
