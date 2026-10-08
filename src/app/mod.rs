#[cfg(test)]
pub(crate) mod benchmark;
mod bookmark;
mod dispatch;
mod display;
pub(crate) use dispatch::WindowState;
mod export;
mod file_ops;
mod image_edit;
mod info;
mod mark;
mod mouse;
mod navigation;
mod open;
mod panel;
mod slideshow;
mod system;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::Receiver;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow, ValidateRect};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::Action;
use crate::archive::ArchiveManager;
use crate::config::Config;
use crate::document::{Document, DocumentEvent};
use crate::extension_registry::ExtensionRegistry;
use crate::image::{DecoderChain, StandardDecoder};
use crate::render::D2DRenderer;
use crate::render::d2d_renderer::DrawOutcome;
use crate::selection::Selection;
use crate::susie::SusieManager;
use crate::ui::cursor_hider::{CursorHider, TIMER_ID_CURSOR_HIDE};
use crate::ui::file_list_panel::FileListPanel;
use crate::ui::font::MonospaceFont;
use crate::ui::fullscreen::FullscreenState;
use crate::ui::key_config::{InputChord, KeyConfig, MouseButton, WheelDirection};
use crate::ui::menu;
use crate::ui::window;

/// DocumentEventをUIスレッドに通知するためのカスタムメッセージ
const WM_DOCUMENT_EVENT: u32 = WM_APP + 1;

use export::ExportFormat;
use slideshow::TIMER_ID_SLIDESHOW;

/// 描画資源の失効から自動で再描画を要求する連続回数の上限
///
/// 再作成した直後に再び失効する環境で、再描画要求が無制限に続くことを防ぐ。
/// 上限到達後は利用者の次の操作による再描画で再試行する。
const MAX_RENDER_RECOVERY_ATTEMPTS: u32 = 3;

/// メインウィンドウ (View 層)
///
/// Win32 メッセージループから呼び出され、`Document` モデルへの操作と `D2DRenderer`
/// による描画を仲介する。`docs/development/architecture.md` の Model-View 分離パターン参照。
///
/// - メニューバー・キー入力・ファイルリストパネル等の UI 状態を所有
/// - `Document` から `DocumentEvent` をチャネル経由で受け取り、再描画や UI 更新を行う
/// - エラーは `show_error_title` でタイトルバーに表示する (詳細は AGENTS.md エラー方針)
pub(crate) struct AppWindow {
    pub(crate) hwnd: HWND,
    effects: std::cell::RefCell<std::collections::VecDeque<dispatch::UiEffect>>,
    pub(crate) document: Document,
    pub(crate) event_receiver: Receiver<DocumentEvent>,
    pub(crate) renderer: D2DRenderer,
    pub(crate) fullscreen: FullscreenState,
    pub(crate) cursor_hider: CursorHider,
    pub(crate) always_on_top: bool,
    pub(crate) keep_titlebar_in_fullscreen: bool,
    pub(crate) key_config: KeyConfig,
    pub(crate) susie_plugin_dir: Option<PathBuf>,
    // メニューバー。menu_visibleはフルスクリーン解除後の表示希望を保持する。
    pub(crate) menu: HMENU,
    pub(crate) menu_visible: bool,
    // ファイルリストパネル
    pub(crate) file_list_panel: FileListPanel,
    // パネル表示中のキャッシュ状態追跡 (差分更新用)
    pub(crate) cached_indices: HashSet<usize>,
    // 等幅フォント (ダイアログ・ファイルリスト用)
    pub(crate) monospace_font: MonospaceFont,
    // 矩形選択
    pub(crate) selection: Selection,
    // ファイル操作の前回使用ディレクトリ (セッション中のみ保持)
    file_operation_directory: file_ops::FileOperationDirectory,
    // スライドショー
    pub(crate) slideshow_active: bool,
    pub(crate) slideshow_interval_ms: u32,
    pub(crate) slideshow_repeat: bool,
    // ブックマーク前回名キャッシュ (セッション中のみ保持)。
    // 値はコンテナ識別キーと前回採用したブックマークファイル名の組。
    // コンテナ切り替え時はコンテナ識別キーの不一致で初期名が自動リセットされる。
    pub(crate) last_bookmark: Option<(PathBuf, String)>,
    // タイトルバーに表示中のエラー。利用者の次の操作の開始まで通常タイトルより優先する。
    // 同じ処理内のリスト更新や選択更新がエラー表示を直後に上書きしないようにするため。
    error_message: Option<String>,
    // 描画資源の失効から連続して描画に成功していない回数
    render_recovery_attempts: u32,
    // 表示中のエラーが描画の失敗か。描画が成功した時点でそのエラーだけを解除する
    render_error_shown: bool,
    // このウィンドウが貼り付けで作成した一時ファイル (ウィンドウ破棄時に回収する)
    pasted_images: file_ops::PastedImageFiles,
    // 試験用: 再描画要求の回数 (非表示ウィンドウでは無効領域を観測できないため)
    #[cfg(test)]
    redraw_request_count: std::cell::Cell<u32>,
}

impl AppWindow {
    /// AppWindowを作成しウィンドウを表示する
    pub fn create(config: Config, initial_files: &[PathBuf]) -> Result<Box<WindowState>> {
        Self::create_with_visibility(config, initial_files, true)
    }

    /// 試験用: 表示しないAppWindowを作成する
    ///
    /// 描画・イベント処理・タイトルバーはウィンドウを表示しなくても実際のWin32/Direct2Dで動作する。
    #[cfg(test)]
    pub(crate) fn create_hidden_for_test() -> Box<WindowState> {
        Self::create_with_visibility(Config::default(), &[], false)
            .expect("hidden AppWindow creation failed")
    }

    fn create_with_visibility(
        config: Config,
        initial_files: &[PathBuf],
        visible: bool,
    ) -> Result<Box<WindowState>> {
        let class_name = windows::core::w!("gv_main");

        // アイコンをリソースからロード (リソースID 1)
        let icon = unsafe {
            let hmodule = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok();
            let hinstance = hmodule.map(|m| windows::Win32::Foundation::HINSTANCE(m.0));
            // MAKEINTRESOURCE(1) — リソースID 1 をポインタとして渡す
            #[allow(clippy::manual_dangling_ptr)]
            LoadIconW(hinstance, windows::core::PCWSTR(1 as *const u16)).ok()
        };
        window::register_window_class_with_icon(class_name, Some(Self::wnd_proc), icon)?;

        let hwnd = window::create_window(class_name, windows::core::w!("ぐらびゅ"), 1024, 768)?;

        // ウィンドウにアイコンを設定 (タスクバー表示用)
        if let Some(ref icon) = icon {
            unsafe {
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(0)), // ICON_SMALL
                    Some(LPARAM(icon.0 as isize)),
                );
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(1)), // ICON_BIG
                    Some(LPARAM(icon.0 as isize)),
                );
            }
        }

        // D&Dを受け付ける
        unsafe {
            DragAcceptFiles(hwnd, true);
        }

        let (sender, receiver) = crossbeam_channel::unbounded();
        let renderer = D2DRenderer::new(hwnd, &config.display)?;

        // 拡張子レジストリ + Susieプラグイン + デコーダチェーン + アーカイブマネージャの初期化
        let mut registry = ExtensionRegistry::new();
        let spi_dir = crate::paths::susie_plugin_dir(&config.susie.plugin_dir).ok();
        let susie_manager = spi_dir
            .as_deref()
            .map(SusieManager::discover)
            .unwrap_or_default();
        susie_manager.register_extensions(&mut registry);

        let registry = Arc::new(registry);

        // デコーダチェーン: Standard → Susie画像プラグイン (フォールバック順)
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

        let document = Document::new(
            sender,
            decoder,
            Arc::clone(&registry),
            archive_manager,
            config.list.default_sort,
        );

        let always_on_top = config.window.always_on_top;
        let keep_titlebar_in_fullscreen = config.window.keep_titlebar_in_fullscreen;
        let base_image_size = config.prefetch.base_image_size();

        // キーバインド設定の読み込み
        let key_config_path = crate::paths::key_config_path().ok();
        let key_config = KeyConfig::load(key_config_path.as_deref());

        // メニューバー構築 (初期状態は非表示)
        let menu_handle = menu::build_menu_bar(&key_config);

        // ファイルリストパネル作成 (初期状態は非表示)
        let file_list_panel = FileListPanel::create(hwnd);

        // 等幅フォント作成 + ファイルリストに適用
        let monospace_font = MonospaceFont::new(16);
        unsafe {
            let _ = SendMessageW(
                file_list_panel.listview_hwnd(),
                WM_SETFONT,
                Some(WPARAM(monospace_font.hfont().0 as usize)),
                Some(LPARAM(1)),
            );
        }

        let app = Self {
            hwnd,
            effects: std::cell::RefCell::default(),
            document,
            event_receiver: receiver,
            renderer,
            fullscreen: FullscreenState::new(),
            cursor_hider: CursorHider::new(),
            always_on_top,
            keep_titlebar_in_fullscreen,
            key_config,
            susie_plugin_dir: spi_dir,
            menu: menu_handle,
            menu_visible: true,
            file_list_panel,
            cached_indices: HashSet::new(),
            monospace_font,
            selection: Selection::new(),
            file_operation_directory: file_ops::FileOperationDirectory::default(),
            slideshow_active: false,
            slideshow_interval_ms: config.slideshow.interval_ms,
            slideshow_repeat: config.slideshow.repeat,
            last_bookmark: None,
            error_message: None,
            render_recovery_attempts: 0,
            render_error_shown: false,
            pasted_images: file_ops::PastedImageFiles::new(std::env::temp_dir()),
            #[cfg(test)]
            redraw_request_count: std::cell::Cell::new(0),
        };
        let state = Box::new(WindowState::new(app));

        // GWLP_USERDATAにポインタを格納 (WndProcからアクセスするため)
        window::set_window_data(hwnd, std::ptr::from_ref(&*state).cast_mut());
        state.with_app(|app| {
            // 先読みエンジン起動
            // 通知コールバック: ワーカースレッドからPostMessageWでUIスレッドを起こす
            let hwnd_raw = hwnd.0 as isize;
            let notify: std::sync::Arc<dyn Fn() + Send + Sync> =
                std::sync::Arc::new(move || unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut _)),
                        WM_DOCUMENT_EVENT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                });
            let cache_budget = Self::get_cache_budget();
            if let Err(e) = app
                .document
                .start_prefetch(notify, cache_budget, base_image_size)
            {
                app.show_error_title(&format!("先読みエンジンの起動に失敗しました: {e:#}"));
            }

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
            if initial_files.is_empty() {
                // ファイル未指定起動: バージョン入りタイトルを反映
                app.update_title();
            } else {
                let result = if initial_files.len() > 1 {
                    // 複数パス: フォルダ・コンテナ・画像・ブックマークの混在をフラットに展開
                    app.document.open_multiple(initial_files)
                } else if initial_files[0].is_dir() {
                    app.document.open_folder(&initial_files[0])
                } else {
                    app.document.open(&initial_files[0])
                };
                if let Err(e) = result {
                    app.show_error_title(&format!("ファイルを開けませんでした: {e:#}"));
                }
                app.process_document_events();
            }
        });
        // メニューバーをデフォルト表示
        let menu_handle = state.borrow().menu;
        unsafe {
            let _ = SetMenu(hwnd, Some(menu_handle));
        }

        if visible {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = UpdateWindow(hwnd);
            }
        }

        Ok(state)
    }

    /// 空きメモリの50%をキャッシュ予算として返す
    fn get_cache_budget() -> usize {
        let mut mem_info = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        let available = unsafe {
            if GlobalMemoryStatusEx(std::ptr::from_mut(&mut mem_info)).is_ok() {
                mem_info.ullAvailPhys as usize
            } else {
                512 * 1024 * 1024 // フォールバック: 512MB
            }
        };
        available / 2
    }

    /// タイトルバーを更新
    ///
    /// エラー表示中は通常タイトルで上書きせず、エラーを表示し続ける。
    fn update_title(&self) {
        let title = if let Some(msg) = &self.error_message {
            format!("ぐらびゅ - エラー: {msg}")
        } else if let Some(source) = self.document.current_source() {
            // PendingContainer 上にいる場合は「読み込み中」プレフィックスを付ける
            let loading_prefix = if source.is_pending_container() {
                "読み込み中: "
            } else {
                ""
            };
            let display = source.display_path();
            let fl = self.document.file_list();
            let page_info = if let Some(idx) = fl.current_index() {
                format!(" [{}/{}]", idx + 1, fl.len())
            } else {
                String::new()
            };
            // ファイルリスト非表示時もマーク状態を確認できるよう、タイトルへ表示する。
            // 記号は ui::file_list_panel のマーク済み表示と揃える。
            let mark_info = if fl.current().is_some_and(|f| f.marked) {
                " ★"
            } else {
                ""
            };
            // 選択情報をタイトルに追加
            let sel_info = if let Some(rect) = self.selection.current_rect() {
                format!(
                    " 選択: ({}, {}) {}×{}",
                    rect.x, rect.y, rect.width, rect.height
                )
            } else {
                String::new()
            };
            // バックグラウンド展開の進捗表示
            let expand_info = if let Some((done, total)) = self.document.expand_progress() {
                format!(" 読込: {done}/{total}")
            } else {
                String::new()
            };
            format!(
                "{loading_prefix}{display}{page_info}{mark_info}{sel_info}{expand_info} - ぐらびゅ"
            )
        } else {
            concat!("ぐらびゅ v", env!("CARGO_PKG_VERSION")).to_string()
        };

        let wide = crate::util::to_wide(title.trim_end_matches('\0'));
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
        });
    }

    /// タイトルバーにエラーメッセージを表示する
    ///
    /// 表示は利用者の次の操作の開始（`begin_user_operation`）まで保持する。
    fn show_error_title(&mut self, msg: &str) {
        self.error_message = Some(msg.to_string());
        self.render_error_shown = false;
        self.update_title();
    }

    /// 描画の失敗を表示する。描画が次に成功した時点で自動的に解除する
    fn show_render_error(&mut self, msg: &str) {
        self.show_error_title(msg);
        self.render_error_shown = true;
    }

    /// 利用者の操作の開始時に呼ぶ。前の操作のエラー表示を解除して通常タイトルへ戻す
    fn begin_user_operation(&mut self) {
        if self.error_message.take().is_some() {
            self.update_title();
        }
    }

    /// 利用者操作の結果を成功・キャンセル・失敗へ分け、失敗だけをタイトルバーへ通知する
    ///
    /// ダイアログはキャンセルを`Ok(None)`で返す。Shell操作の中止（`Ok(false)`）は
    /// `.map(|done| done.then_some(()))`で同じ形へそろえて渡す。
    /// 失敗をキャンセルと同じ「何もしない」へまとめると利用者へ届かないため、分類をここへ集約する。
    /// 成功時だけ値を返すので、呼出側は`Some`のときだけ保存先記憶やリスト更新を行う。
    fn take_success<T>(&mut self, operation: &str, result: Result<Option<T>>) -> Option<T> {
        match result {
            Ok(value) => value,
            Err(e) => {
                self.show_error_title(&format!("{operation}に失敗しました: {e:#}"));
                None
            }
        }
    }

    /// モーダルダイアログ表示前にカーソルを可視化する
    ///
    /// フルスクリーンモードでのカーソル自動非表示状態から開くダイアログでも
    /// カーソルを可視状態にする。ダイアログ呼び出しの直前で呼ぶ。
    fn prepare_modal_dialog(&mut self) {
        self.cursor_hider.force_show(self.hwnd);
    }

    /// モーダルダイアログ終了後にタイマーを再セットする
    ///
    /// フルスクリーン中で自動非表示が有効なら非表示タイマーを再起動する。
    /// ダイアログ呼び出しの直後で呼ぶ。
    fn finish_modal_dialog(&mut self) {
        if self.fullscreen.is_fullscreen() && self.cursor_hider.is_enabled() {
            self.cursor_hider.on_mouse_move(self.hwnd);
        }
    }

    /// 再描画をリクエスト
    fn invalidate(&self) {
        #[cfg(test)]
        self.redraw_request_count
            .set(self.redraw_request_count.get() + 1);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    // --- WndProc ---

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: create_with_visibilityが登録したBox<WindowState>は、メッセージループと
        // 同期コールバックの終了まで生存する。WM_DESTROYで登録を解除する。
        // 共有参照だけを生成し、AppWindowの排他的アクセスはRefCellが管理する。
        let state = unsafe {
            crate::ui::dialog::get_window_data::<WindowState>(hwnd).map(|ptr| ptr.as_ref())
        };
        if let Some(state) = state
            && let Some(result) = state.dispatch(msg, wparam, lparam)
        {
            return result;
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn handle_message(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        let app = self;
        match msg {
            WM_PAINT => {
                app.on_paint();
                return Some(LRESULT(0));
            }
            WM_SIZE => {
                let width = (lparam.0 & 0xFFFF) as u32;
                let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                app.on_size(width, height);
                return Some(LRESULT(0));
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                // Escキー: ドラッグ操作中は選択をキャンセル (key_configより優先)
                if wparam.0 as u16 == 0x1B && app.selection.is_dragging() {
                    app.selection.deselect();
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture()
                            .unwrap_or_default();
                    }
                    app.invalidate();
                    app.update_title();
                    return Some(LRESULT(0));
                }

                let chord = InputChord::Key {
                    vk: wparam.0 as u16,
                    modifiers: Self::current_modifiers(),
                };
                if let Some(action) = app.key_config.lookup(chord) {
                    app.execute_action(action);
                    return Some(LRESULT(0));
                }
                // SYSKEYDOWNは未処理時にDefWindowProcへ渡す必要あり
                if msg == WM_SYSKEYDOWN {
                    // fall through to DefWindowProcW
                } else {
                    return Some(LRESULT(0));
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
                if let Some(action) = app.key_config.lookup(chord) {
                    app.execute_action(action);
                    // 同期再描画でフレームスキップ防止
                    let hwnd = app.hwnd;
                    app.defer_ui(move || unsafe {
                        let _ = UpdateWindow(hwnd);
                    });
                }
                return Some(LRESULT(0));
            }
            WM_LBUTTONDOWN => {
                app.on_lbutton_down(lparam);
                return Some(LRESULT(0));
            }
            WM_LBUTTONUP => {
                app.on_lbutton_up();
                return Some(LRESULT(0));
            }
            WM_LBUTTONDBLCLK => {
                let chord = InputChord::Mouse {
                    button: MouseButton::LeftDoubleClick,
                };
                if let Some(action) = app.key_config.lookup(chord) {
                    app.execute_action(action);
                }
                return Some(LRESULT(0));
            }
            WM_MBUTTONUP => {
                let chord = InputChord::Mouse {
                    button: MouseButton::MiddleClick,
                };
                if let Some(action) = app.key_config.lookup(chord) {
                    app.execute_action(action);
                }
                return Some(LRESULT(0));
            }
            WM_MOUSEMOVE => {
                if app.fullscreen.is_fullscreen() {
                    app.cursor_hider.on_mouse_move(hwnd);
                }
                app.on_mouse_move(lparam);
                return Some(LRESULT(0));
            }
            WM_SETCURSOR => {
                // 選択状態に応じてカーソルを変更
                if app.on_set_cursor() {
                    return Some(LRESULT(1));
                }
            }
            WM_TIMER => {
                if wparam.0 == TIMER_ID_CURSOR_HIDE {
                    app.cursor_hider.on_timer(hwnd);
                    return Some(LRESULT(0));
                }
                if wparam.0 == TIMER_ID_SLIDESHOW {
                    app.on_slideshow_timer();
                    return Some(LRESULT(0));
                }
            }
            WM_INITMENUPOPUP => {
                // wParam = 開こうとしているポップアップメニューのHMENU
                let popup = HMENU(wparam.0 as *mut _);
                app.update_menu_checks(popup);
            }
            WM_COMMAND => {
                let notify_code = ((wparam.0 as u32) >> 16) & 0xFFFF;
                let control_id = (wparam.0 as u32) & 0xFFFF;
                let control_hwnd = HWND(lparam.0 as *mut _);

                // メニュー項目 (notify_code == 0 かつコントロールなし)
                if notify_code == 0 && control_hwnd.0.is_null() {
                    if let Some(action) = menu::menu_id_to_action(control_id as u16) {
                        app.execute_action(action);
                    }
                    return Some(LRESULT(0));
                }

                return Some(LRESULT(0));
            }
            WM_NOTIFY => {
                // SAFETY: WM_NOTIFY の lparam は OS が有効な NMHDR へのポインタを保証する
                let (source, code) = unsafe {
                    let header = &*(lparam.0 as *const NMHDR);
                    (header.hwndFrom, header.code)
                };
                if source == app.file_list_panel.listview_hwnd() {
                    return Some(app.handle_file_list_notify(code, lparam));
                }
                return Some(LRESULT(0));
            }
            WM_DROPFILES => {
                app.on_drop_files(HDROP(wparam.0 as *mut _));
                return Some(LRESULT(0));
            }
            WM_ERASEBKGND => {
                // Direct2Dが背景を描画するのでちらつき防止
                return Some(LRESULT(1));
            }
            msg if msg == WM_DOCUMENT_EVENT => {
                app.process_document_events();
                return Some(LRESULT(0));
            }
            _ => {}
        }

        None
    }

    fn on_paint(&mut self) {
        let outcome = self.paint();
        // WM_PAINTの無限ループを防ぐためにValidateRectを呼ぶ
        // (再描画が必要な場合はこの後のinvalidateで改めて要求する)
        unsafe {
            let _ = ValidateRect(Some(self.hwnd), None);
        }
        self.handle_paint_outcome(outcome);
    }

    /// 現在の画像と選択範囲を描画する
    fn paint(&mut self) -> Result<DrawOutcome> {
        let sel_rect = self.selection.current_rect();
        self.renderer
            .draw(self.document.current_image(), sel_rect.as_ref())
    }

    /// 描画結果に応じて再描画要求・失敗通知を行う
    ///
    /// 描画資源の失効では再描画を要求し、次の描画で資源を再作成する。
    /// 連続回数の上限到達と描画エラーでは通知だけを行い、自動の再描画要求を止める。
    fn handle_paint_outcome(&mut self, outcome: Result<DrawOutcome>) {
        match outcome {
            Ok(DrawOutcome::Drawn) => {
                self.render_recovery_attempts = 0;
                // 復旧した描画エラーは表示し続けない (他の操作のエラーは残す)
                if std::mem::take(&mut self.render_error_shown) {
                    self.error_message = None;
                    self.update_title();
                }
            }
            Ok(DrawOutcome::TargetLost) => {
                self.render_recovery_attempts += 1;
                if self.render_recovery_attempts <= MAX_RENDER_RECOVERY_ATTEMPTS {
                    self.invalidate();
                } else {
                    self.show_render_error(
                        "描画を復旧できませんでした。ウィンドウサイズの変更などで再描画すると再試行します",
                    );
                }
            }
            Err(e) => self.show_render_error(&format!("描画に失敗しました: {e:#}")),
        }
    }

    pub(super) fn on_size(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            // ファイルリストパネルのリサイズ
            let panel_width = self.file_list_panel.panel_width() as u32;
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.resize(height as i32));

            // D2Dレンダーターゲットは全体サイズでリサイズ
            if let Err(e) = self.renderer.resize(width, height) {
                self.show_render_error(&format!("{e:#}"));
            }

            // 描画オフセットを設定 (パネル幅分だけ右にずらす)
            self.renderer.set_draw_offset(panel_width as f32);

            self.invalidate();
        }
    }

    /// アクションを実行する
    fn execute_action(&mut self, action: Action) {
        self.begin_user_operation();

        // スライドショー中は、スライドショー関連以外のアクションで自動停止
        if self.slideshow_active
            && !matches!(
                action,
                Action::SlideshowToggle | Action::SlideshowFaster | Action::SlideshowSlower
            )
        {
            self.stop_slideshow();
        }

        match action {
            // --- ナビゲーション ---
            Action::NavigateBack => self.navigate_with_guard(|d| d.navigate_relative(-1)),
            Action::NavigateForward => self.navigate_with_guard(|d| d.navigate_relative(1)),
            Action::Navigate5Back => self.navigate_with_guard(|d| d.navigate_relative(-5)),
            Action::Navigate5Forward => self.navigate_with_guard(|d| d.navigate_relative(5)),
            Action::Navigate50Back => self.navigate_with_guard(|d| d.navigate_relative(-50)),
            Action::Navigate50Forward => self.navigate_with_guard(|d| d.navigate_relative(50)),
            Action::NavigateFirst => self.navigate_with_guard(Document::navigate_first),
            Action::NavigateLast => self.navigate_with_guard(Document::navigate_last),

            // --- 表示モード ---
            Action::DisplayAutoShrink => self.action_display_auto_shrink(),
            Action::DisplayAutoFit => self.action_display_auto_fit(),
            Action::ZoomIn => self.action_zoom_in(),
            Action::ZoomOut => self.action_zoom_out(),
            Action::ZoomReset => self.action_zoom_reset(),
            Action::ToggleMargin => self.action_toggle_margin(),
            Action::CycleAlphaBackground => self.action_cycle_alpha_background(),

            // --- ウィンドウ ---
            Action::ToggleFullscreen => self.toggle_fullscreen(),
            Action::Minimize => self.action_minimize(),
            Action::ToggleMaximize => self.action_toggle_maximize(),
            Action::ToggleAlwaysOnTop => self.toggle_always_on_top(),
            Action::ToggleCursorHide => self.action_toggle_cursor_hide(),

            // --- マーク操作 ---
            Action::MarkSet => self.action_mark_set(),
            Action::MarkUnset => self.action_mark_unset(),
            Action::MarkInvertAll => self.action_mark_invert_all(),
            Action::MarkInvertToHere => self.action_mark_invert_to_here(),
            Action::NavigatePrevMark => self.navigate_with_guard(Document::navigate_prev_mark),
            Action::NavigateNextMark => self.navigate_with_guard(Document::navigate_next_mark),
            Action::RemoveFromList => self.action_remove_from_list(),
            Action::MarkedRemoveFromList => self.action_marked_remove_from_list(),

            // --- フォルダナビゲーション ---
            Action::NavigatePrevFolder => self.navigate_with_guard(Document::navigate_prev_folder),
            Action::NavigateNextFolder => self.navigate_with_guard(Document::navigate_next_folder),

            // --- ファイル操作 ---
            Action::OpenFile => self.action_open_file(),
            Action::OpenFolder => self.action_open_folder(),
            Action::DeleteFile => self.action_delete_file(),
            Action::MoveFile => self.action_move_file(),
            Action::CopyFile => self.action_copy_file(),
            Action::MarkedDelete => self.action_marked_delete(),
            Action::MarkedMove => self.action_marked_move(),
            Action::MarkedCopy => self.action_marked_copy(),
            Action::Reload => self.navigate_with_guard(Document::reload),

            // --- クリップボード ---
            Action::CopyImage => self.action_copy_image(),
            Action::CopyFileName => self.action_copy_file_name(),
            Action::MarkedCopyNames => self.action_marked_copy_names(),
            Action::PasteImage => self.action_paste_image(),

            // --- 画像保存 ---
            Action::ExportJpg => self.export_image(ExportFormat::Jpg),
            Action::ExportBmp => self.export_image(ExportFormat::Bmp),
            Action::ExportPng => self.export_image(ExportFormat::Png),

            // --- ユーティリティ ---
            Action::NewWindow => self.action_new_window(),
            Action::CloseAll => self.action_close_all(),
            Action::OpenContainingFolder => self.action_open_containing_folder(),
            Action::OpenExeFolder => self.action_open_exe_folder(),
            Action::OpenBookmarkFolder => self.action_open_bookmark_folder(),
            Action::OpenSpiFolder => self.action_open_spi_folder(),
            Action::OpenTempFolder => self.action_open_temp_folder(),
            Action::ShowImageInfo => self.show_image_info(),

            // --- 編集 ---
            Action::DeselectSelection => self.action_deselect_selection(),
            Action::Crop => self.action_crop(),
            Action::RotateArbitrary => self.action_rotate_arbitrary(),
            Action::Resize => self.action_resize(),

            Action::PFilterToggle => self.action_p_filter_toggle(),

            // --- ブックマーク ---
            Action::BookmarkSave => self.action_bookmark_save(),
            Action::BookmarkLoad => self.action_bookmark_load(),
            // --- ページ指定ナビゲーション ---
            Action::NavigateToPage => self.action_navigate_to_page(),

            // --- ソートナビゲーション ---
            Action::SortNavigateBack => self.navigate_with_guard(Document::sort_navigate_back),
            Action::SortNavigateForward => self.action_sort_navigate_forward(),

            // --- シャッフル ---
            Action::ShuffleAll => self.action_shuffle_all(),
            Action::ShuffleGroups => self.action_shuffle_groups(),

            // --- メニューバー ---
            Action::ToggleMenuBar => self.action_toggle_menu_bar(),

            // --- ファイルリスト ---
            Action::ToggleFileList => self.action_toggle_file_list(),

            // --- ヘルプ ---
            Action::ShowHelp => self.show_help(),

            // --- アップデート ---
            Action::CheckUpdate => self.check_for_update(),

            // --- ホームページ ---
            Action::OpenHomepage => self.action_open_homepage(),

            // --- シェル統合 ---
            Action::RegisterShell => self.action_register_shell(),
            Action::UnregisterShell => self.action_unregister_shell(),

            // --- スライドショー ---
            Action::SlideshowToggle => self.toggle_slideshow(),
            Action::SlideshowFaster => self.adjust_slideshow_interval(-500),
            Action::SlideshowSlower => self.adjust_slideshow_interval(500),

            // --- 終了 ---
            Action::Exit => self.action_exit(),
            _ => self.action_filter_for_action(action),
        }
    }
}
