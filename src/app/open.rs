//! 開く・閉じる・新規ウィンドウ・ドロップとエクスプローラー操作

use super::AppWindow;
use anyhow::Context as _;
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;

impl AppWindow {
    pub(super) fn action_open_file(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        let initial_dir = self
            .document
            .current_source()
            .and_then(|s| s.parent_dir())
            .map(Path::to_path_buf);
        let hwnd = self.hwnd;
        let registry = self.document.file_list().registry_handle();
        self.defer_modal(
            move || {
                crate::ui::file_dialog::open_file_dialog(hwnd, initial_dir.as_deref(), &registry)
            },
            |app, result| {
                let Some(path) = app.take_success("ファイル選択ダイアログの表示", result)
                else {
                    return;
                };
                match app.document.open(&path) {
                    Ok(()) => app.file_operation_directory.reset(),
                    Err(e) => app.show_error_title(&format!("ファイルを開けませんでした: {e:#}")),
                }
                app.process_document_events();
            },
        );
    }

    pub(super) fn action_open_folder(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        let initial_dir = self
            .document
            .current_source()
            .and_then(|s| s.parent_dir())
            .map(Path::to_path_buf);
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::file_dialog::open_folder_dialog(hwnd, initial_dir.as_deref()),
            |app, result| {
                let Some(path) = app.take_success("フォルダ選択ダイアログの表示", result)
                else {
                    return;
                };
                match app.document.open_folder(&path) {
                    Ok(()) => app.file_operation_directory.reset(),
                    Err(e) => app.show_error_title(&format!("フォルダを開けませんでした: {e:#}")),
                }
                app.process_document_events();
            },
        );
    }

    pub(super) fn action_new_window(&mut self) {
        // 引数なしで空のウィンドウを起動
        let result = crate::paths::exe_path().and_then(|exe| {
            std::process::Command::new(&exe)
                .spawn()
                .map(|_| ())
                .context("プロセスを起動できませんでした")
        });
        if let Err(e) = result {
            self.show_error_title(&format!("新規ウィンドウの起動に失敗しました: {e:#}"));
        }
    }

    pub(super) fn action_close_all(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        self.document.close_all();
        self.file_operation_directory.reset();
        self.process_document_events();
        self.update_title();
    }

    pub(super) fn open_in_explorer_select(&mut self, path: &Path) {
        let arg = format!("/select,{}", path.display());
        if let Err(e) = std::process::Command::new("explorer.exe")
            .raw_arg(&arg)
            .spawn()
        {
            self.show_error_title(&format!("エクスプローラの起動に失敗しました: {e:#}"));
        }
    }

    pub(super) fn open_in_explorer(&mut self, path: &Path) {
        if let Err(e) = std::process::Command::new("explorer.exe").arg(path).spawn() {
            self.show_error_title(&format!("エクスプローラの起動に失敗しました: {e:#}"));
        }
    }

    pub(super) fn action_open_containing_folder(&mut self) {
        if let Some(source) = self.document.current_source() {
            let target = match source {
                crate::file_info::FileSource::ArchiveEntry { archive, .. } => archive.clone(),
                crate::file_info::FileSource::PdfPage { pdf_path, .. } => pdf_path.clone(),
                crate::file_info::FileSource::PendingContainer { container_path } => {
                    container_path.clone()
                }
                crate::file_info::FileSource::File(path) => path.clone(),
            };
            self.open_in_explorer_select(&target);
        }
    }

    pub(super) fn on_drop_files(&mut self, hdrop: HDROP) {
        self.begin_user_operation();
        self.before_image_change();
        self.selection.deselect();

        // ドロップされた全ファイルを収集
        let file_count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) } as usize;
        let mut paths = Vec::new();
        let mut buf = [0u16; 1024];
        for i in 0..file_count {
            let len = unsafe { DragQueryFileW(hdrop, i as u32, Some(&mut buf)) } as usize;
            if len > 0 {
                let path_str = String::from_utf16_lossy(&buf[..len]);
                paths.push(std::path::PathBuf::from(path_str));
            }
        }
        unsafe { DragFinish(hdrop) };

        if paths.is_empty() {
            return;
        }

        let result = if paths.len() > 1 {
            // 複数パス: フォルダ・コンテナ・画像の混在をすべてフラットに展開
            self.document.open_multiple(&paths)
        } else if paths[0].is_dir() {
            self.document.open_folder(&paths[0])
        } else {
            self.document.open(&paths[0])
        };

        match result {
            Ok(()) => self.file_operation_directory.reset(),
            Err(e) => {
                self.show_error_title(&format!("ドロップされたファイルを開けませんでした: {e:#}"));
            }
        }

        self.process_document_events();
    }

    pub(super) fn action_exit(&mut self) {
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let _ = DestroyWindow(hwnd);
        });
    }
}
