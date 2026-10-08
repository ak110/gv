//! システム操作アクション群
//!
//! アップデート確認、シェル統合登録・解除、各種フォルダ・ホームページを開く操作。

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::AppWindow;

/// フォルダが無ければ作成してから返す
fn ensure_dir(dir: PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("フォルダを作成できませんでした: {}", dir.display()))?;
    Ok(dir)
}

impl AppWindow {
    /// アップデート確認・実行
    pub(crate) fn check_for_update(&mut self) {
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                // WaitCursor表示
                let prev_cursor = unsafe { SetCursor(LoadCursorW(None, IDC_WAIT).ok()) };

                let result = crate::updater::check_for_update();

                // カーソル復元
                unsafe {
                    let _ = SetCursor(Some(prev_cursor));
                }

                match result {
                    Err(e) => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "アップデート確認",
                            &format!("更新の確認に失敗しました:\n{e:#}"),
                            MB_OK | MB_ICONERROR,
                        );
                    },
                    Ok(info) if !info.is_newer => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "アップデート確認",
                            &format!("最新バージョンです (v{})", info.current_version),
                            MB_OK | MB_ICONINFORMATION,
                        );
                    },
                    Ok(info) => {
                        let answer = unsafe {
                            crate::util::show_message_box(
                                hwnd,
                                "アップデート確認",
                                &format!(
                                    "v{} が利用可能です (現在: v{})。\n更新しますか？",
                                    info.latest_version, info.current_version
                                ),
                                MB_YESNO | MB_ICONQUESTION,
                            )
                        };

                        if answer == IDYES {
                            // WaitCursor表示
                            let prev = unsafe { SetCursor(LoadCursorW(None, IDC_WAIT).ok()) };

                            match crate::updater::perform_update(&info) {
                                Ok(true) => {
                                    // バッチスクリプト起動成功 → アプリ終了
                                    unsafe {
                                        let _ = SetCursor(Some(prev));
                                        let _ = DestroyWindow(hwnd);
                                    }
                                }
                                Ok(false) => unsafe {
                                    let _ = SetCursor(Some(prev));
                                },
                                Err(e) => {
                                    unsafe {
                                        let _ = SetCursor(Some(prev));
                                    }
                                    unsafe {
                                        crate::util::show_message_box(
                                            hwnd,
                                            "アップデート",
                                            &format!("更新に失敗しました:\n{e:#}"),
                                            MB_OK | MB_ICONERROR,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            },
            |_, ()| {},
        );
    }

    /// シェル統合 (ファイル関連付け・コンテキストメニュー・「送る」) を登録
    pub(crate) fn action_register_shell(&mut self) {
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                let answer = unsafe {
                    crate::util::show_message_box(
                        hwnd,
                        "シェル統合",
                        "ファイル関連付け・コンテキストメニュー・「送る」を登録しますか？",
                        MB_YESNO | MB_ICONQUESTION,
                    )
                };
                if answer != IDYES {
                    return;
                }

                match crate::shell::register_all() {
                    Ok(()) => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "シェル統合",
                            "シェル統合を登録しました。",
                            MB_OK | MB_ICONINFORMATION,
                        );
                    },
                    Err(e) => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "シェル統合",
                            &format!("シェル統合の登録に失敗しました:\n{e:#}"),
                            MB_OK | MB_ICONERROR,
                        );
                    },
                }
            },
            |_, ()| {},
        );
    }

    /// シェル統合 (ファイル関連付け・コンテキストメニュー・「送る」) を解除
    pub(crate) fn action_unregister_shell(&mut self) {
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                let answer = unsafe {
                    crate::util::show_message_box(
                        hwnd,
                        "シェル統合",
                        "ファイル関連付け・コンテキストメニュー・「送る」を解除しますか？",
                        MB_YESNO | MB_ICONQUESTION,
                    )
                };
                if answer != IDYES {
                    return;
                }

                match crate::shell::unregister_all() {
                    Ok(()) => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "シェル統合",
                            "シェル統合を解除しました。",
                            MB_OK | MB_ICONINFORMATION,
                        );
                    },
                    Err(e) => unsafe {
                        crate::util::show_message_box(
                            hwnd,
                            "シェル統合",
                            &format!("シェル統合の解除に失敗しました:\n{e:#}"),
                            MB_OK | MB_ICONERROR,
                        );
                    },
                }
            },
            |_, ()| {},
        );
    }

    pub(crate) fn action_open_exe_folder(&mut self) {
        self.open_folder_in_explorer("実行ファイルのフォルダを開く操作", crate::paths::exe_dir());
    }

    pub(crate) fn action_open_bookmark_folder(&mut self) {
        let dir = ensure_dir(crate::bookmark::bookmark_dir());
        self.open_folder_in_explorer("ブックマークフォルダを開く操作", dir);
    }

    pub(crate) fn action_open_spi_folder(&mut self) {
        let dir = self
            .susie_plugin_dir
            .clone()
            .context("Susieプラグインの配置を解決できませんでした")
            .and_then(ensure_dir);
        self.open_folder_in_explorer("spiフォルダを開く操作", dir);
    }

    /// フォルダの解決に成功した場合だけエクスプローラーで開き、失敗は通知する
    fn open_folder_in_explorer(&mut self, operation: &str, dir: Result<PathBuf>) {
        if let Some(dir) = self.take_success(operation, dir.map(Some)) {
            self.open_in_explorer(&dir);
        }
    }

    pub(crate) fn action_open_temp_folder(&mut self) {
        let dir = std::env::temp_dir();
        self.open_in_explorer(&dir);
    }

    pub(crate) fn action_open_homepage(&mut self) {
        let hwnd = self.hwnd;
        self.defer_call(
            move || {
                let url = windows::core::w!("https://github.com/ak110/gv");
                let result = unsafe {
                    windows::Win32::UI::Shell::ShellExecuteW(
                        Some(hwnd),
                        windows::core::PCWSTR::null(),
                        url,
                        windows::core::PCWSTR::null(),
                        windows::core::PCWSTR::null(),
                        SW_SHOWNORMAL,
                    )
                };
                result.0 as isize
            },
            |app, code| {
                // ShellExecuteW は戻り値が32以下の場合エラー（WinSDK仕様）
                if code <= 32 {
                    app.show_error_title(&format!("ブラウザの起動に失敗しました: {code}"));
                }
            },
        );
    }
}
