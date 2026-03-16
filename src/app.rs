use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::Receiver;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow, ValidateRect};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::archive::ArchiveManager;
use crate::config::Config;
use crate::document::{Document, DocumentEvent};
use crate::extension_registry::ExtensionRegistry;
use crate::image::{DecoderChain, StandardDecoder};
use crate::render::D2DRenderer;
use crate::render::layout::DisplayMode;
use crate::susie::SusieManager;
use crate::ui::cursor_hider::{CursorHider, TIMER_ID_CURSOR_HIDE};
use crate::ui::fullscreen::FullscreenState;
use crate::ui::key_config::{
    Action, InputChord, KeyConfig, Modifiers, MouseButton, WheelDirection,
};
use crate::ui::window;

/// DocumentEventをUIスレッドに通知するためのカスタムメッセージ
const WM_DOCUMENT_EVENT: u32 = WM_APP + 1;

/// 修飾キーVKコード
const VK_CONTROL: i32 = 0x11;
const VK_SHIFT: i32 = 0x10;
const VK_MENU: i32 = 0x12; // Alt

/// メインウィンドウ
pub struct AppWindow {
    hwnd: HWND,
    document: Document,
    event_receiver: Receiver<DocumentEvent>,
    renderer: D2DRenderer,
    fullscreen: FullscreenState,
    cursor_hider: CursorHider,
    always_on_top: bool,
    key_config: KeyConfig,
}

impl AppWindow {
    /// AppWindowを作成しウィンドウを表示する
    pub fn create(config: Config, initial_file: Option<&Path>) -> Result<Box<Self>> {
        let class_name = windows::core::w!("gv3_main");
        window::register_window_class(class_name, Some(Self::wnd_proc))?;

        let hwnd = window::create_window(class_name, windows::core::w!("ぐらびゅ3"), 1024, 768)?;

        // D&Dを受け付ける
        unsafe {
            DragAcceptFiles(hwnd, true);
        }

        let (sender, receiver) = crossbeam_channel::unbounded();
        let renderer = D2DRenderer::new(hwnd, &config.display)?;

        // 拡張子レジストリ + Susieプラグイン + デコーダチェーン + アーカイブマネージャの初期化
        let mut registry = ExtensionRegistry::new();
        let spi_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(&config.susie.plugin_dir)));
        let susie_manager = spi_dir
            .as_deref()
            .map(SusieManager::discover)
            .unwrap_or_default();
        susie_manager.register_extensions(&mut registry);

        let registry = Arc::new(registry);

        // デコーダチェーン: Standard → Susie画像プラグイン（フォールバック順）
        let mut decoders: Vec<Box<dyn crate::image::ImageDecoder>> =
            vec![Box::new(StandardDecoder::new())];
        for decoder in susie_manager.create_image_decoders() {
            decoders.push(decoder);
        }
        let decoder = Arc::new(DecoderChain::new(decoders));

        // アーカイブマネージャ + Susieアーカイブプラグイン
        let mut archive_manager = ArchiveManager::new(Arc::clone(&registry));
        for handler in susie_manager.create_archive_handlers(Arc::clone(&registry)) {
            archive_manager.add_handler(handler);
        }

        let document = Document::new(sender, decoder, Arc::clone(&registry), archive_manager);

        let always_on_top = config.window.always_on_top;
        let base_image_size = config.prefetch.base_image_size();

        // キーバインド設定の読み込み
        let key_config_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("gv3.keys.toml")));
        let key_config = KeyConfig::load(key_config_path.as_deref());

        let mut app = Box::new(Self {
            hwnd,
            document,
            event_receiver: receiver,
            renderer,
            fullscreen: FullscreenState::new(),
            cursor_hider: CursorHider::new(),
            always_on_top,
            key_config,
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
        app.document
            .start_prefetch(notify, cache_budget, base_image_size);

        // 設定でalways_on_topが有効な場合、ウィンドウに反映
        if always_on_top {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }

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
        let title = if let Some(source) = self.document.current_source() {
            let display = source.display_path();
            // 表示名: ファイル名部分を抽出
            let filename = display
                .rsplit(['/', '\\', '>'])
                .next()
                .map(|s| s.trim())
                .unwrap_or("???");
            let fl = self.document.file_list();
            // アーカイブ名をタイトルに付加
            let archive_suffix = source
                .archive_path()
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

    /// 現在の修飾キー状態を取得
    fn current_modifiers() -> Modifiers {
        unsafe {
            Modifiers {
                ctrl: GetKeyState(VK_CONTROL) < 0,
                shift: GetKeyState(VK_SHIFT) < 0,
                alt: GetKeyState(VK_MENU) < 0,
            }
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
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let chord = InputChord::Key {
                        vk: wparam.0 as u16,
                        modifiers: Self::current_modifiers(),
                    };
                    if let Some(action) = app.key_config.lookup(&chord) {
                        app.execute_action(action);
                        return LRESULT(0);
                    }
                    // SYSKEYDOWNは未処理時にDefWindowProcへ渡す必要あり
                    if msg == WM_SYSKEYDOWN {
                        // fall through to DefWindowProcW
                    } else {
                        return LRESULT(0);
                    }
                }
                WM_MOUSEWHEEL => {
                    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                    let direction = if delta > 0 {
                        WheelDirection::Up
                    } else {
                        WheelDirection::Down
                    };
                    let chord = InputChord::Wheel {
                        direction,
                        modifiers: Self::current_modifiers(),
                    };
                    if let Some(action) = app.key_config.lookup(&chord) {
                        app.execute_action(action);
                    }
                    return LRESULT(0);
                }
                WM_LBUTTONDBLCLK => {
                    let chord = InputChord::Mouse {
                        button: MouseButton::LeftDoubleClick,
                    };
                    if let Some(action) = app.key_config.lookup(&chord) {
                        app.execute_action(action);
                    }
                    return LRESULT(0);
                }
                WM_MBUTTONUP => {
                    let chord = InputChord::Mouse {
                        button: MouseButton::MiddleClick,
                    };
                    if let Some(action) = app.key_config.lookup(&chord) {
                        app.execute_action(action);
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

    /// アクションを実行する
    fn execute_action(&mut self, action: Action) {
        match action {
            // --- ナビゲーション ---
            Action::NavigateBack => {
                self.document.navigate_relative(-1);
                self.process_document_events();
            }
            Action::NavigateForward => {
                self.document.navigate_relative(1);
                self.process_document_events();
            }
            Action::Navigate1Back => {
                self.document.navigate_relative(-1);
                self.process_document_events();
            }
            Action::Navigate1Forward => {
                self.document.navigate_relative(1);
                self.process_document_events();
            }
            Action::Navigate5Back => {
                self.document.navigate_relative(-5);
                self.process_document_events();
            }
            Action::Navigate5Forward => {
                self.document.navigate_relative(5);
                self.process_document_events();
            }
            Action::Navigate50Back => {
                self.document.navigate_relative(-50);
                self.process_document_events();
            }
            Action::Navigate50Forward => {
                self.document.navigate_relative(50);
                self.process_document_events();
            }
            Action::NavigateFirst => {
                self.document.navigate_first();
                self.process_document_events();
            }
            Action::NavigateLast => {
                self.document.navigate_last();
                self.process_document_events();
            }

            // --- 表示モード ---
            Action::DisplayAutoShrink => {
                self.renderer.layout_mut().mode = DisplayMode::AutoShrink;
                self.invalidate();
            }
            Action::DisplayAutoFit => {
                self.renderer.layout_mut().mode = DisplayMode::AutoFit;
                self.invalidate();
            }
            Action::ZoomIn => {
                if let Some((iw, ih)) = self.current_image_size() {
                    let (ww, wh) = self.client_size();
                    self.renderer.layout_mut().zoom_in(iw, ih, ww, wh);
                    self.invalidate();
                }
            }
            Action::ZoomOut => {
                if let Some((iw, ih)) = self.current_image_size() {
                    let (ww, wh) = self.client_size();
                    self.renderer.layout_mut().zoom_out(iw, ih, ww, wh);
                    self.invalidate();
                }
            }
            Action::ZoomReset => {
                self.renderer.layout_mut().zoom_reset();
                self.invalidate();
            }
            Action::ToggleMargin => {
                self.renderer.layout_mut().toggle_margin();
                self.invalidate();
            }
            Action::CycleAlphaBackground => {
                self.renderer.cycle_alpha_background();
                self.invalidate();
            }

            // --- ウィンドウ ---
            Action::ToggleFullscreen => {
                self.toggle_fullscreen();
            }
            Action::Minimize => unsafe {
                let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            },
            Action::ToggleMaximize => {
                if !self.fullscreen.is_fullscreen() {
                    self.toggle_maximize();
                }
            }
            Action::ToggleAlwaysOnTop => {
                self.toggle_always_on_top();
            }
            Action::ToggleCursorHide => {
                self.cursor_hider.toggle_enabled(self.hwnd);
            }

            // --- マーク操作 ---
            Action::MarkSet => {
                self.document.mark_current();
                self.process_document_events();
            }
            Action::MarkUnset => {
                self.document.unmark_current();
            }
            Action::MarkInvertAll => {
                self.document.invert_all_marks();
            }
            Action::MarkInvertToHere => {
                self.document.invert_marks_to_here();
            }
            Action::NavigatePrevMark => {
                self.document.navigate_prev_mark();
                self.process_document_events();
            }
            Action::NavigateNextMark => {
                self.document.navigate_next_mark();
                self.process_document_events();
            }
            Action::RemoveFromList => {
                self.document.remove_current_from_list();
                self.process_document_events();
            }
            Action::MarkedRemoveFromList => {
                self.document.remove_marked_from_list();
                self.process_document_events();
            }

            // --- フォルダナビゲーション ---
            Action::NavigatePrevFolder => {
                self.document.navigate_prev_folder();
                self.process_document_events();
            }
            Action::NavigateNextFolder => {
                self.document.navigate_next_folder();
                self.process_document_events();
            }

            // --- ファイル操作 ---
            Action::OpenFile => {
                if let Ok(Some(path)) = crate::file_ops::open_file_dialog(self.hwnd) {
                    if let Err(e) = self.document.open(&path) {
                        eprintln!("ファイルを開けませんでした: {e}");
                    }
                    self.process_document_events();
                }
            }
            Action::OpenFolder => {
                if let Ok(Some(path)) = crate::file_ops::open_folder_dialog(self.hwnd) {
                    if let Err(e) = self.document.open_folder(&path) {
                        eprintln!("フォルダを開けませんでした: {e}");
                    }
                    self.process_document_events();
                }
            }
            Action::DeleteFile => {
                // アーカイブ内ファイルの削除は無効
                if let Some(source) = self.document.current_source()
                    && source.is_archive_entry()
                {
                    return;
                }
                if let Some(path) = self.document.current_path().map(|p| p.to_path_buf()) {
                    if let Ok(true) = crate::file_ops::delete_to_recycle_bin(self.hwnd, &[&path]) {
                        self.document.remove_current_from_list();
                        self.process_document_events();
                    }
                }
            }
            Action::MoveFile => {
                if let Some(source) = self.document.current_source()
                    && source.is_archive_entry()
                {
                    return;
                }
                if let Some(path) = self.document.current_path().map(|p| p.to_path_buf()) {
                    if let Ok(Some(dest)) =
                        crate::file_ops::select_folder_dialog(self.hwnd, "移動先フォルダ")
                    {
                        if let Ok(true) = crate::file_ops::move_files(self.hwnd, &[&path], &dest) {
                            self.document.remove_current_from_list();
                            self.process_document_events();
                        }
                    }
                }
            }
            Action::CopyFile => {
                if let Some(path) = self.document.current_path().map(|p| p.to_path_buf()) {
                    let default_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                    if let Ok(Some(dest)) = crate::file_ops::save_file_dialog(
                        self.hwnd,
                        default_name,
                        "すべてのファイル",
                        "*.*",
                    ) {
                        if let Err(e) = std::fs::copy(&path, &dest) {
                            eprintln!("ファイルコピー失敗: {e}");
                        }
                    }
                }
            }
            Action::MarkedDelete => {
                // アーカイブ内は無効
                if self.document.current_archive().is_some() {
                    return;
                }
                let paths: Vec<std::path::PathBuf> = self
                    .document
                    .file_list()
                    .marked_indices()
                    .iter()
                    .map(|&i| self.document.file_list().files()[i].path.clone())
                    .collect();
                let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                if let Ok(true) = crate::file_ops::delete_to_recycle_bin(self.hwnd, &path_refs) {
                    self.document.remove_marked_from_list();
                    self.process_document_events();
                }
            }
            Action::MarkedMove => {
                if self.document.current_archive().is_some() {
                    return;
                }
                let paths: Vec<std::path::PathBuf> = self
                    .document
                    .file_list()
                    .marked_indices()
                    .iter()
                    .map(|&i| self.document.file_list().files()[i].path.clone())
                    .collect();
                if paths.is_empty() {
                    return;
                }
                if let Ok(Some(dest)) =
                    crate::file_ops::select_folder_dialog(self.hwnd, "移動先フォルダ")
                {
                    let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    if let Ok(true) = crate::file_ops::move_files(self.hwnd, &path_refs, &dest) {
                        self.document.remove_marked_from_list();
                        self.process_document_events();
                    }
                }
            }
            Action::MarkedCopy => {
                let paths: Vec<std::path::PathBuf> = self
                    .document
                    .file_list()
                    .marked_indices()
                    .iter()
                    .map(|&i| self.document.file_list().files()[i].path.clone())
                    .collect();
                if paths.is_empty() {
                    return;
                }
                if let Ok(Some(dest)) =
                    crate::file_ops::select_folder_dialog(self.hwnd, "コピー先フォルダ")
                {
                    let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    if let Err(e) = crate::file_ops::copy_files(self.hwnd, &path_refs, &dest) {
                        eprintln!("ファイルコピー失敗: {e}");
                    }
                }
            }
            Action::Reload => {
                self.document.reload();
                self.process_document_events();
            }

            // --- クリップボード ---
            Action::CopyImage => {
                if let Some(image) = self.document.current_image() {
                    if let Err(e) = crate::clipboard::copy_image_to_clipboard(self.hwnd, image) {
                        eprintln!("画像コピー失敗: {e}");
                    }
                }
            }
            Action::CopyFileName => {
                if let Some(source) = self.document.current_source() {
                    if let Err(e) =
                        crate::clipboard::copy_text_to_clipboard(self.hwnd, &source.display_path())
                    {
                        eprintln!("ファイル名コピー失敗: {e}");
                    }
                }
            }
            Action::MarkedCopyNames => {
                let names: Vec<String> = self
                    .document
                    .file_list()
                    .marked_indices()
                    .iter()
                    .map(|&i| self.document.file_list().files()[i].source.display_path())
                    .collect();
                if !names.is_empty() {
                    let text = names.join("\r\n");
                    if let Err(e) = crate::clipboard::copy_text_to_clipboard(self.hwnd, &text) {
                        eprintln!("マークファイル名コピー失敗: {e}");
                    }
                }
            }
            Action::PasteImage => {
                match crate::clipboard::paste_image_from_clipboard(self.hwnd) {
                    Ok(Some(image)) => {
                        // 一時ファイルに保存してから開く
                        let temp_path = std::env::temp_dir().join("gv3_clipboard.png");
                        if let Some(img_buf) = image::RgbaImage::from_raw(
                            image.width,
                            image.height,
                            image.data.clone(),
                        ) {
                            if img_buf.save(&temp_path).is_ok() {
                                if let Err(e) = self.document.open(&temp_path) {
                                    eprintln!("貼り付け失敗: {e}");
                                }
                                self.process_document_events();
                            }
                        }
                    }
                    Ok(None) => {} // クリップボードに画像なし
                    Err(e) => eprintln!("貼り付け失敗: {e}"),
                }
            }

            // --- 画像書き出し ---
            Action::ExportJpg => {
                self.export_image("jpg", "JPEG画像", "*.jpg");
            }
            Action::ExportBmp => {
                self.export_image("bmp", "BMP画像", "*.bmp");
            }
            Action::ExportPng => {
                self.export_image("png", "PNG画像", "*.png");
            }

            // --- ユーティリティ ---
            Action::NewWindow => {
                // 現在のexeを新規プロセスで起動
                if let Ok(exe) = std::env::current_exe() {
                    let mut cmd = std::process::Command::new(&exe);
                    // アーカイブ内の場合はアーカイブパスを渡す
                    if let Some(source) = self.document.current_source() {
                        match source {
                            crate::file_info::FileSource::ArchiveEntry { archive, .. } => {
                                cmd.arg(archive);
                            }
                            crate::file_info::FileSource::File(path) => {
                                cmd.arg(path);
                            }
                        }
                    }
                    let _ = cmd.spawn();
                }
            }
            Action::CloseAll => {
                self.document.close_all();
                self.process_document_events();
            }
            Action::OpenContainingFolder => {
                if let Some(source) = self.document.current_source() {
                    let target = match source {
                        crate::file_info::FileSource::ArchiveEntry { archive, .. } => {
                            archive.clone()
                        }
                        crate::file_info::FileSource::File(path) => path.clone(),
                    };
                    // /select,path でエクスプローラを開く
                    let arg = format!("/select,{}", target.display());
                    let _ = std::process::Command::new("explorer.exe").arg(&arg).spawn();
                }
            }
            Action::OpenExeFolder => {
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(dir) = exe.parent() {
                        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
                    }
                }
            }
            Action::OpenBookmarkFolder => {
                let dir = crate::bookmark::bookmark_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
            }
            Action::OpenSpiFolder => {
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(dir) = exe.parent() {
                        let spi_dir = dir.join("spi");
                        let _ = std::fs::create_dir_all(&spi_dir);
                        let _ = std::process::Command::new("explorer.exe")
                            .arg(&spi_dir)
                            .spawn();
                    }
                }
            }
            Action::OpenTempFolder => {
                let dir = std::env::temp_dir();
                let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
            }
            Action::ShowImageInfo => {
                self.show_image_info();
            }

            // --- ブックマーク ---
            Action::BookmarkSave => {
                let idx = self.document.file_list().current_index();
                if let Err(e) =
                    crate::bookmark::save_bookmark(self.hwnd, self.document.file_list(), idx)
                {
                    eprintln!("ブックマーク保存失敗: {e}");
                }
            }
            Action::BookmarkLoad => {
                match crate::bookmark::load_bookmark(self.hwnd) {
                    Ok(Some(data)) => {
                        if let Err(e) = self.document.load_bookmark_data(data) {
                            eprintln!("ブックマーク読み込み失敗: {e}");
                        }
                        self.process_document_events();
                    }
                    Ok(None) => {} // キャンセル
                    Err(e) => eprintln!("ブックマーク読み込み失敗: {e}"),
                }
            }
            Action::BookmarkOpenEditor => {
                let dir = crate::bookmark::bookmark_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
            }

            // --- ページ指定ナビゲーション ---
            Action::NavigateToPage => {
                self.navigate_to_page_dialog();
            }

            // --- Phase 8 スタブ（未実装） ---
            Action::SortNavigateBack
            | Action::SortNavigateForward
            | Action::ToggleMenuBar
            | Action::OpenHistory
            | Action::ToggleFileList
            | Action::DialogDisplay
            | Action::DialogImage
            | Action::DialogDraw
            | Action::DialogList
            | Action::DialogGeneral
            | Action::DialogPlugin
            | Action::DialogEnvironment
            | Action::DialogKeys
            | Action::ShowHelp => {
                eprintln!("未実装: {action:?}");
            }
        }
    }

    /// ページ指定ナビゲーション
    /// 総ページ数の桁数に応じて、数字キー入力でページ移動する簡易方式
    fn navigate_to_page_dialog(&mut self) {
        let total = self.document.file_list().len();
        if total == 0 {
            return;
        }
        let current = self.document.file_list().current_index().unwrap_or(0) + 1;

        // MessageBoxで現在位置と総数を表示して入力を促す
        // （本格的な入力ダイアログは設定ダイアログフェーズで実装予定）
        let prompt = format!(
            "現在: {current} / {total}\n\nページ番号をタイトルバーに入力してEnterで移動\n（この機能は設定ダイアログフェーズで改善予定）\0"
        );
        let title = "ページ指定移動\0";
        let wide_prompt: Vec<u16> = prompt.encode_utf16().collect();
        let wide_title: Vec<u16> = title.encode_utf16().collect();
        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                windows::core::PCWSTR(wide_prompt.as_ptr()),
                windows::core::PCWSTR(wide_title.as_ptr()),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        // TODO: 設定ダイアログフェーズで入力ダイアログに置き換え
    }

    /// 画像を指定フォーマットで書き出す
    fn export_image(&mut self, ext: &str, filter_name: &str, filter_spec: &str) {
        let Some(img) = self.document.current_image() else {
            return;
        };
        let default_name = self
            .document
            .current_path()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| format!("{s}.{ext}"))
            .unwrap_or_else(|| format!("image.{ext}"));

        let Some(save_path) =
            crate::file_ops::save_file_dialog(self.hwnd, &default_name, filter_name, filter_spec)
                .ok()
                .flatten()
        else {
            return;
        };

        // DecodedImage (RGBA) → image::RgbaImage → encode
        let Some(img_buf) = image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
        else {
            eprintln!("画像バッファ作成失敗");
            return;
        };

        if let Err(e) = img_buf.save(&save_path) {
            eprintln!("画像書き出し失敗: {e}");
        }
    }

    /// 画像情報を表示する
    fn show_image_info(&self) {
        let Some(source) = self.document.current_source() else {
            return;
        };
        let Some(file_info) = self.document.file_list().current() else {
            return;
        };

        let mut info_lines = Vec::new();
        info_lines.push(format!("パス: {}", source.display_path()));
        info_lines.push(format!("ファイルサイズ: {} bytes", file_info.file_size));

        if let Some(img) = self.document.current_image() {
            info_lines.push(format!("画像サイズ: {} x {}", img.width, img.height));
        }

        // メタデータ取得（デコーダ経由）
        if let Ok(metadata) = self.document.current_metadata() {
            info_lines.push(format!("フォーマット: {}", metadata.format));
            for comment in &metadata.comments {
                info_lines.push(format!("コメント: {comment}"));
            }
        }

        let text = info_lines.join("\n");
        let title = "画像情報\0";
        let text_nul = format!("{text}\0");
        let wide_title: Vec<u16> = title.encode_utf16().collect();
        let wide_text: Vec<u16> = text_nul.encode_utf16().collect();

        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                windows::core::PCWSTR(wide_text.as_ptr()),
                windows::core::PCWSTR(wide_title.as_ptr()),
                MB_OK | MB_ICONINFORMATION,
            );
        }
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
