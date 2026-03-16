use std::path::Path;

use anyhow::Result;
use crossbeam_channel::Receiver;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow, ValidateRect};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::document::{Document, DocumentEvent};
use crate::render::D2DRenderer;
use crate::render::layout::DisplayMode;
use crate::ui::cursor_hider::{CursorHider, TIMER_ID_CURSOR_HIDE};
use crate::ui::fullscreen::FullscreenState;
use crate::ui::window;

/// DocumentEventをUIスレッドに通知するためのカスタムメッセージ
const WM_DOCUMENT_EVENT: u32 = WM_APP + 1;

/// 仮想キーコード定数
const VK_LEFT: i32 = 0x25;
const VK_RIGHT: i32 = 0x27;
const VK_PRIOR: i32 = 0x21; // PageUp
const VK_NEXT: i32 = 0x22; // PageDown
const VK_HOME: i32 = 0x24;
const VK_END: i32 = 0x23;
const VK_RETURN: i32 = 0x0D;
const VK_CONTROL: i32 = 0x11;
const VK_X: i32 = 0x58;
const VK_T: i32 = 0x54;
const VK_A: i32 = 0x41;
const VK_NUMPAD0: i32 = 0x60;
const VK_ADD: i32 = 0x6B;
const VK_SUBTRACT: i32 = 0x6D;
const VK_MULTIPLY: i32 = 0x6A;
const VK_DIVIDE: i32 = 0x6F;

/// メインウィンドウ
pub struct AppWindow {
    hwnd: HWND,
    document: Document,
    event_receiver: Receiver<DocumentEvent>,
    renderer: D2DRenderer,
    fullscreen: FullscreenState,
    cursor_hider: CursorHider,
    always_on_top: bool,
}

impl AppWindow {
    /// AppWindowを作成しウィンドウを表示する
    pub fn create(initial_file: Option<&Path>) -> Result<Box<Self>> {
        let class_name = windows::core::w!("gv3_main");
        window::register_window_class(class_name, Some(Self::wnd_proc))?;

        let hwnd = window::create_window(class_name, windows::core::w!("ぐらびゅ3"), 1024, 768)?;

        // D&Dを受け付ける
        unsafe {
            DragAcceptFiles(hwnd, true);
        }

        let (sender, receiver) = crossbeam_channel::unbounded();
        let renderer = D2DRenderer::new(hwnd)?;
        let document = Document::new(sender);

        let mut app = Box::new(Self {
            hwnd,
            document,
            event_receiver: receiver,
            renderer,
            fullscreen: FullscreenState::new(),
            cursor_hider: CursorHider::new(),
            always_on_top: false,
        });

        // GWLP_USERDATAにポインタを格納（WndProcからアクセスするため）
        window::set_window_data(hwnd, &mut *app as *mut Self);

        // 先読みエンジン起動
        // 通知コールバック: ワーカースレッドからPostMessageWでUIスレッドを起こす
        let hwnd_raw = hwnd.0 as isize;
        let notify = Box::new(move || unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_raw as *mut _)),
                WM_DOCUMENT_EVENT,
                WPARAM(0),
                LPARAM(0),
            );
        });
        let cache_budget = Self::get_cache_budget();
        app.document.start_prefetch(notify, cache_budget);

        // 初期ファイルがあれば開く
        if let Some(path) = initial_file {
            if let Err(e) = app.document.open(path) {
                eprintln!("画像を開けませんでした: {e}");
            }
            app.process_document_events();
        }

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        Ok(app)
    }

    /// 空きメモリの50%をキャッシュ予算として返す
    fn get_cache_budget() -> usize {
        let mut mem_info = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        let available = unsafe {
            if GlobalMemoryStatusEx(&mut mem_info).is_ok() {
                mem_info.ullAvailPhys as usize
            } else {
                512 * 1024 * 1024 // フォールバック: 512MB
            }
        };
        available / 2
    }

    /// DocumentEventを処理する
    fn process_document_events(&mut self) {
        // 先読みレスポンスを処理（キャッシュ格納 + current_image更新）
        self.document.process_prefetch_responses();

        while let Ok(event) = self.event_receiver.try_recv() {
            match event {
                DocumentEvent::ImageReady => unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                },
                DocumentEvent::NavigationChanged { .. } => {
                    self.update_title();
                }
                DocumentEvent::FileListChanged => {}
                DocumentEvent::Error(msg) => {
                    eprintln!("エラー: {msg}");
                }
            }
        }
    }

    /// タイトルバーを更新
    fn update_title(&self) {
        let title = if let Some(path) = self.document.current_path() {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("???");
            let fl = self.document.file_list();
            // アーカイブ名をタイトルに付加
            let archive_suffix = self
                .document
                .current_archive()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|name| format!(" [{name}]"))
                .unwrap_or_default();
            if let Some(idx) = fl.current_index() {
                format!(
                    "{filename} ({}/{}{archive_suffix}) - ぐらびゅ3\0",
                    idx + 1,
                    fl.len()
                )
            } else {
                format!("{filename}{archive_suffix} - ぐらびゅ3\0")
            }
        } else {
            "ぐらびゅ3\0".to_string()
        };

        let wide: Vec<u16> = title.encode_utf16().collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd, windows::core::PCWSTR(wide.as_ptr()));
        }
    }

    /// Ctrlキーが押されているか判定
    fn is_ctrl_down() -> bool {
        unsafe { GetKeyState(VK_CONTROL) < 0 }
    }

    /// Altキーが押されているか判定
    fn is_alt_down() -> bool {
        unsafe {
            GetKeyState(0x12 /* VK_MENU */) < 0
        }
    }

    /// 再描画をリクエスト
    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// 現在の画像サイズを返す（zoom操作用）
    fn current_image_size(&self) -> Option<(u32, u32)> {
        self.document
            .current_image()
            .map(|img| (img.width, img.height))
    }

    /// クライアント領域のサイズを返す
    fn client_size(&self) -> (f32, f32) {
        let (w, h) = window::get_client_size(self.hwnd);
        (w as f32, h as f32)
    }

    /// 常に手前に表示をトグル
    fn toggle_always_on_top(&mut self) {
        self.always_on_top = !self.always_on_top;
        // フルスクリーン中は復帰時に反映されるので今は何もしない
        if !self.fullscreen.is_fullscreen() {
            let z_order = if self.always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(z_order),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }

    /// フルスクリーンをトグル
    fn toggle_fullscreen(&mut self) {
        self.fullscreen.toggle(self.hwnd, self.always_on_top);
        if !self.fullscreen.is_fullscreen() {
            // フルスクリーン解除時にカーソルを確実に復帰
            self.cursor_hider.force_show(self.hwnd);
        }
    }

    /// 最大化トグル（左ダブルクリック）
    fn toggle_maximize(&self) {
        unsafe {
            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            let _ = GetWindowPlacement(self.hwnd, &mut placement);
            if placement.showCmd == SW_MAXIMIZE.0 as u32 {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(self.hwnd, SW_MAXIMIZE);
            }
        }
    }

    // --- WndProc ---

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if let Some(app) = window::get_window_data::<Self>(hwnd) {
            match msg {
                WM_PAINT => {
                    app.on_paint();
                    return LRESULT(0);
                }
                WM_SIZE => {
                    let width = (lparam.0 & 0xFFFF) as u32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    app.on_size(width, height);
                    return LRESULT(0);
                }
                WM_KEYDOWN => {
                    app.on_keydown(wparam.0 as i32);
                    return LRESULT(0);
                }
                WM_SYSKEYDOWN => {
                    // Alt+Enter → フルスクリーントグル
                    if wparam.0 as i32 == VK_RETURN && Self::is_alt_down() {
                        app.toggle_fullscreen();
                        return LRESULT(0);
                    }
                }
                WM_MOUSEWHEEL => {
                    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                    let ctrl = Self::is_ctrl_down();
                    if ctrl {
                        // Ctrl+ホイール → 倍率変更
                        if let Some((iw, ih)) = app.current_image_size() {
                            let (ww, wh) = app.client_size();
                            let layout = app.renderer.layout_mut();
                            if delta > 0 {
                                layout.zoom_out(iw, ih, ww, wh);
                            } else {
                                layout.zoom_in(iw, ih, ww, wh);
                            }
                            app.invalidate();
                        }
                        return LRESULT(0);
                    }
                }
                WM_LBUTTONDBLCLK => {
                    if !app.fullscreen.is_fullscreen() {
                        app.toggle_maximize();
                    }
                    return LRESULT(0);
                }
                WM_MOUSEMOVE => {
                    if app.fullscreen.is_fullscreen() {
                        app.cursor_hider.on_mouse_move(hwnd);
                    }
                    return LRESULT(0);
                }
                WM_TIMER => {
                    if wparam.0 == TIMER_ID_CURSOR_HIDE {
                        app.cursor_hider.on_timer(hwnd);
                        return LRESULT(0);
                    }
                }
                WM_DROPFILES => {
                    app.on_drop_files(HDROP(wparam.0 as *mut _));
                    return LRESULT(0);
                }
                WM_ERASEBKGND => {
                    // Direct2Dが背景を描画するのでちらつき防止
                    return LRESULT(1);
                }
                WM_DESTROY => {
                    // ポインタをクリアしてダングリング参照を防止
                    window::set_window_data::<Self>(hwnd, std::ptr::null_mut());
                    unsafe { PostQuitMessage(0) };
                    return LRESULT(0);
                }
                msg if msg == WM_DOCUMENT_EVENT => {
                    app.process_document_events();
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn on_paint(&mut self) {
        self.renderer.draw(self.document.current_image());
        // WM_PAINTの無限ループを防ぐためにValidateRectを呼ぶ
        unsafe {
            let _ = ValidateRect(Some(self.hwnd), None);
        }
    }

    fn on_size(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.renderer.resize(width, height);
            self.invalidate();
        }
    }

    fn on_keydown(&mut self, vk: i32) {
        let ctrl = Self::is_ctrl_down();

        match (vk, ctrl) {
            // --- ナビゲーション ---
            (VK_LEFT, false) => self.document.navigate_relative(-1),
            (VK_RIGHT, false) => self.document.navigate_relative(1),
            (VK_PRIOR, false) => self.document.navigate_relative(-5),
            (VK_NEXT, false) => self.document.navigate_relative(5),
            (VK_PRIOR, true) => self.document.navigate_relative(-50),
            (VK_NEXT, true) => self.document.navigate_relative(50),
            (VK_HOME, true) => self.document.navigate_first(),
            (VK_END, true) => self.document.navigate_last(),

            // --- 表示モード切替 ---
            (VK_DIVIDE, false) => {
                // Num / → 自動縮小
                self.renderer.layout_mut().mode = DisplayMode::AutoShrink;
                self.invalidate();
                return;
            }
            (VK_MULTIPLY, false) => {
                // Num * → 自動拡大・縮小
                self.renderer.layout_mut().mode = DisplayMode::AutoFit;
                self.invalidate();
                return;
            }

            // --- 倍率変更 ---
            (VK_SUBTRACT, true) => {
                // Ctrl+Num - → zoom out
                if let Some((iw, ih)) = self.current_image_size() {
                    let (ww, wh) = self.client_size();
                    self.renderer.layout_mut().zoom_out(iw, ih, ww, wh);
                    self.invalidate();
                }
                return;
            }
            (VK_ADD, true) => {
                // Ctrl+Num + → zoom in
                if let Some((iw, ih)) = self.current_image_size() {
                    let (ww, wh) = self.client_size();
                    self.renderer.layout_mut().zoom_in(iw, ih, ww, wh);
                    self.invalidate();
                }
                return;
            }
            (VK_NUMPAD0, true) => {
                // Ctrl+Num0 → 倍率リセット
                self.renderer.layout_mut().zoom_reset();
                self.invalidate();
                return;
            }

            // --- 余白トグル ---
            (VK_NUMPAD0, false) => {
                self.renderer.layout_mut().toggle_margin();
                self.invalidate();
                return;
            }

            // --- カーソル非表示トグル ---
            (VK_SUBTRACT, false) => {
                // Num - → カーソル非表示トグル
                self.cursor_hider.toggle_enabled(self.hwnd);
                return;
            }

            // --- ウィンドウ操作 ---
            (VK_X, true) => {
                // Ctrl+X → 最小化
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
                }
                return;
            }
            (VK_T, true) => {
                // Ctrl+T → 常に手前に表示トグル
                self.toggle_always_on_top();
                return;
            }

            // --- αチャネル背景切替 ---
            (VK_A, false) => {
                // A → White → Black → Checker 巡回
                self.renderer.cycle_alpha_background();
                self.invalidate();
                return;
            }

            _ => return,
        }

        // ナビゲーション操作後のイベント処理
        self.process_document_events();
    }

    fn on_drop_files(&mut self, hdrop: HDROP) {
        // ドロップされた最初のファイル/フォルダのパスを取得
        let mut buf = [0u16; 1024];
        let len = unsafe { DragQueryFileW(hdrop, 0, Some(&mut buf)) } as usize;
        unsafe { DragFinish(hdrop) };

        if len == 0 {
            return;
        }

        let path_str = String::from_utf16_lossy(&buf[..len]);
        let path = std::path::Path::new(&path_str);

        let result = if path.is_dir() {
            self.document.open_folder(path)
        } else {
            self.document.open(path)
        };

        if let Err(e) = result {
            eprintln!("ドロップされたファイルを開けませんでした: {e}");
        }

        self.process_document_events();
    }
}
