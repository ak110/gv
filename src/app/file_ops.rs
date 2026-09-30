//! ファイル操作アクション群
//!
//! 削除、移動、コピー、マーク操作、クリップボード貼り付けなど。
//! 各操作の結果は`AppWindow::take_success`で成功・キャンセル・失敗へ分け、失敗だけを通知する。

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use super::AppWindow;
use crate::image::DecodedImage;

#[derive(Default)]
pub(super) struct FileOperationDirectory {
    path: Option<PathBuf>,
}

impl FileOperationDirectory {
    pub(super) fn initial(&self, fallback: Option<&Path>) -> Option<PathBuf> {
        self.path
            .clone()
            .or_else(|| fallback.map(Path::to_path_buf))
    }

    pub(super) fn remember_folder(&mut self, folder: &Path) {
        self.path = Some(folder.to_path_buf());
    }

    pub(super) fn remember_file(&mut self, file: &Path) {
        if let Some(parent) = file.parent() {
            self.remember_folder(parent);
        }
    }

    pub(super) fn reset(&mut self) {
        self.path = None;
    }
}

/// ウィンドウが貼り付けで作成した一時ファイル
///
/// 貼り付けごとに一意なファイルを新規作成し、作成したウィンドウだけが所有・回収する。
/// 固定名で共有すると、別ウィンドウや後の貼り付けが、表示中の画像の再読込・コピー・出力が
/// 読む内容を置き換えるため。表示・再読込・出力は通常ファイルとして既存の経路で扱う。
pub(super) struct PastedImageFiles {
    dir: PathBuf,
    files: Vec<PathBuf>,
    next_seq: u64,
    /// 試験経路: 次の書き込みを失敗させる
    #[cfg(test)]
    fail_next_write: bool,
}

impl PastedImageFiles {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            files: Vec::new(),
            next_seq: 0,
            #[cfg(test)]
            fail_next_write: false,
        }
    }

    /// 画像をPNGとして新しい一時ファイルへ保存し、そのパスを返す
    ///
    /// 失敗した場合は書き込み途中のファイルだけを削除し、以前に保存したファイルは残す。
    pub(super) fn save(&mut self, image: &DecodedImage) -> Result<PathBuf> {
        let img_buf = image::RgbaImage::from_raw(image.width, image.height, image.data.clone())
            .context("画像バッファの作成に失敗しました")?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img_buf)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .context("PNGへの変換に失敗しました")?;

        let (path, mut file) = self.create_unique_file()?;
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_write) {
            // 作成済みのファイルへ一部だけ書いた状態で失敗させる
            let _ = file.write_all(&png[..png.len() / 2]);
            png.clear();
            png.extend_from_slice(b"unused");
            drop(file);
            file = OpenOptions::new().read(true).open(&path)?;
        }
        if let Err(e) = file.write_all(&png).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(anyhow::Error::from(e).context(format!(
                "一時ファイルへの書き込みに失敗しました: {}",
                path.display()
            )));
        }
        self.files.push(path.clone());
        Ok(path)
    }

    /// 既存ファイルを上書きしない新規作成専用のオープンで、一意な名前のファイルを作成する
    fn create_unique_file(&mut self) -> Result<(PathBuf, File)> {
        let pid = std::process::id();
        loop {
            let path = self
                .dir
                .join(format!("gv_paste_{pid}_{}.png", self.next_seq));
            self.next_seq += 1;
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                // 同じ名前が残っている (同じPIDだった過去のプロセスなど) 場合は次の番号を試す
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e).context(format!(
                        "一時ファイルを作成できませんでした: {}",
                        self.dir.display()
                    )));
                }
            }
        }
    }

    /// このウィンドウが作成した一時ファイルだけを削除する (ウィンドウ破棄時の後始末)
    pub(super) fn remove_all(&mut self) {
        for path in self.files.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl AppWindow {
    pub(crate) fn action_delete_file(&mut self) {
        // コンテナ内 (アーカイブ/PDF) のファイル削除は無効
        if let Some(source) = self.document.current_source()
            && source.is_contained()
        {
            return;
        }
        if let Some(path) = self.document.current_path().map(Path::to_path_buf) {
            self.prepare_modal_dialog();
            let delete_result = crate::file_ops::delete_to_recycle_bin(self.hwnd, &[&path])
                .map(|done| done.then_some(()));
            self.finish_modal_dialog();
            if self.take_success("ファイルの削除", delete_result).is_some() {
                self.document.remove_current_from_list();
                self.process_document_events();
            }
            // Shell APIがフォーカスを奪うことがあるため復帰
            unsafe {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub(crate) fn action_move_file(&mut self) {
        let Some(current) = self.document.file_list().current() else {
            return;
        };
        let source = current.source.clone();
        let path = current.path.clone();

        // PDFページ・未展開コンテナは移動不可
        if matches!(
            source,
            crate::file_info::FileSource::PdfPage { .. }
                | crate::file_info::FileSource::PendingContainer { .. }
        ) {
            return;
        }

        let source_dir = source.parent_dir();
        let initial_dir = self.file_operation_directory.initial(source_dir);
        let default_name = source.default_save_name();

        // ファイルソースに応じてダイアログのラベルを分岐
        let (dialog_title, dialog_button) = match &source {
            crate::file_info::FileSource::File(_) => ("ファイルを移動", "移動"),
            _ => ("ファイルを保存", "保存"),
        };

        self.prepare_modal_dialog();
        let dialog_result = crate::file_ops::save_file_dialog(
            self.hwnd,
            crate::file_ops::SaveFileDialogParams {
                default_name: &default_name,
                filter_name: "すべてのファイル",
                filter_ext: "*.*",
                initial_dir: initial_dir.as_deref(),
                title: Some(dialog_title),
                ok_button_label: Some(dialog_button),
                ..Default::default()
            },
        );
        self.finish_modal_dialog();
        if let Some(dest) = self.take_success("保存ダイアログの表示", dialog_result) {
            match &source {
                crate::file_info::FileSource::File(_) => {
                    // 通常ファイル: SHFileOperationWでUndo対応の移動
                    match crate::file_ops::move_single_file(self.hwnd, &path, &dest) {
                        Ok(true) => {
                            self.file_operation_directory.remember_file(&dest);
                            if let Err(e) = self.document.rename_current_in_list(&dest) {
                                self.show_error_title(&format!("リストの更新に失敗しました: {e}"));
                            }
                            self.process_document_events();
                        }
                        Ok(false) => {} // ユーザーキャンセル
                        Err(e) => {
                            self.show_error_title(&format!("ファイルの移動に失敗しました: {e}"));
                        }
                    }
                    // Shell APIがフォーカスを奪うことがあるため復帰
                    unsafe {
                        let _ = SetForegroundWindow(self.hwnd);
                    }
                }
                crate::file_info::FileSource::ArchiveEntry { on_demand, .. } => {
                    // アーカイブエントリ: 保存 (リスト除去なし)
                    let result = if *on_demand {
                        self.document
                            .read_file_data_current()
                            .and_then(|data| crate::file_ops::write_atomic(&dest, &data))
                    } else {
                        crate::file_ops::copy_atomic(&path, &dest)
                    };
                    match result {
                        Ok(()) => self.file_operation_directory.remember_file(&dest),
                        Err(e) => {
                            self.show_error_title(&format!("ファイルの保存に失敗しました: {e}"));
                        }
                    }
                }
                crate::file_info::FileSource::PdfPage { .. }
                | crate::file_info::FileSource::PendingContainer { .. } => {
                    unreachable!(); // 上でガード済み
                }
            }
        }
    }

    pub(crate) fn action_copy_file(&mut self) {
        // ダイアログ前後で self への可変借用を要求するため、
        // current の借用スコープはダイアログ前で閉じ、必要値は所有値へ複製する。
        let (default_name, source_dir, source, path) = {
            let Some(current) = self.document.file_list().current() else {
                return;
            };
            (
                current.source.default_save_name(),
                current.source.parent_dir().map(Path::to_path_buf),
                current.source.clone(),
                current.path.clone(),
            )
        };
        let initial_dir = self.file_operation_directory.initial(source_dir.as_deref());
        self.prepare_modal_dialog();
        let dialog_result = crate::file_ops::save_file_dialog(
            self.hwnd,
            crate::file_ops::SaveFileDialogParams {
                default_name: &default_name,
                filter_name: "すべてのファイル",
                filter_ext: "*.*",
                initial_dir: initial_dir.as_deref(),
                title: Some("ファイルを複製"),
                ok_button_label: Some("複製"),
                ..Default::default()
            },
        );
        self.finish_modal_dialog();
        if let Some(dest) = self.take_success("保存ダイアログの表示", dialog_result) {
            let result = if matches!(
                source,
                crate::file_info::FileSource::ArchiveEntry {
                    on_demand: true,
                    ..
                }
            ) {
                // オンデマンド: アーカイブから読み込んで保存
                self.document
                    .read_file_data_current()
                    .and_then(|data| crate::file_ops::write_atomic(&dest, &data))
            } else {
                crate::file_ops::copy_atomic(&path, &dest)
            };
            match result {
                Ok(()) => self.file_operation_directory.remember_file(&dest),
                Err(e) => {
                    self.show_error_title(&format!("ファイルのコピーに失敗しました: {e}"));
                }
            }
        }
    }

    pub(crate) fn action_marked_delete(&mut self) {
        // コンテナ内 (アーカイブ/PDF) は無効
        if let Some(source) = self.document.current_source()
            && source.is_contained()
        {
            return;
        }
        let paths: Vec<PathBuf> = self
            .document
            .file_list()
            .marked_indices()
            .iter()
            .map(|&i| self.document.file_list().files()[i].path.clone())
            .collect();
        let path_refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        self.prepare_modal_dialog();
        let delete_result = crate::file_ops::delete_to_recycle_bin(self.hwnd, &path_refs)
            .map(|done| done.then_some(()));
        self.finish_modal_dialog();
        if self
            .take_success("マークファイルの削除", delete_result)
            .is_some()
        {
            self.document.remove_marked_from_list();
            self.process_document_events();
        }
        // Shell APIがフォーカスを奪うことがあるため復帰
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub(crate) fn action_marked_move(&mut self) {
        if let Some(source) = self.document.current_source()
            && source.is_contained()
        {
            return;
        }
        let marked = self.document.file_list().marked_indices();
        let paths: Vec<PathBuf> = marked
            .iter()
            .map(|&i| self.document.file_list().files()[i].path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let source_dir = self.document.file_list().files()[marked[0]]
            .source
            .parent_dir()
            .map(Path::to_path_buf);
        let initial_dir = self.file_operation_directory.initial(source_dir.as_deref());
        self.prepare_modal_dialog();
        let dialog_result = crate::file_ops::select_folder_dialog(
            self.hwnd,
            "移動先フォルダ",
            initial_dir.as_deref(),
        );
        self.finish_modal_dialog();
        if let Some(dest) = self.take_success("フォルダ選択ダイアログの表示", dialog_result)
        {
            let path_refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            let move_result = crate::file_ops::move_files(self.hwnd, &path_refs, &dest)
                .map(|done| done.then_some(()));
            if self.take_success("ファイルの移動", move_result).is_some() {
                self.file_operation_directory.remember_folder(&dest);
                // パス更新失敗時はリストから削除する (移動済みのパスを表示し続けないためのフォールバック)
                if let Err(e) = self.document.update_marked_paths(&dest) {
                    self.document.remove_marked_from_list();
                    self.show_error_title(&format!(
                        "移動後のリスト更新に失敗したため、移動したファイルをリストから除外しました: {e:#}"
                    ));
                }
                self.process_document_events();
            }
            // Shell APIがフォーカスを奪うことがあるため復帰
            unsafe {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub(crate) fn action_marked_copy(&mut self) {
        let marked = self.document.file_list().marked_indices();
        let paths: Vec<PathBuf> = marked
            .iter()
            .map(|&i| self.document.file_list().files()[i].path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let source_dir = self.document.file_list().files()[marked[0]]
            .source
            .parent_dir()
            .map(Path::to_path_buf);
        let initial_dir = self.file_operation_directory.initial(source_dir.as_deref());
        self.prepare_modal_dialog();
        let dialog_result = crate::file_ops::select_folder_dialog(
            self.hwnd,
            "コピー先フォルダ",
            initial_dir.as_deref(),
        );
        self.finish_modal_dialog();
        if let Some(dest) = self.take_success("フォルダ選択ダイアログの表示", dialog_result)
        {
            let path_refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            match crate::file_ops::copy_files(self.hwnd, &path_refs, &dest) {
                Ok(true) => self.file_operation_directory.remember_folder(&dest),
                Ok(false) => {}
                Err(e) => {
                    self.show_error_title(&format!("ファイルのコピーに失敗しました: {e}"));
                }
            }
            // Shell APIがフォーカスを奪うことがあるため復帰
            unsafe {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub(crate) fn action_marked_copy_names(&mut self) {
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
                self.show_error_title(&format!("マークファイル名のコピーに失敗しました: {e}"));
            }
        }
    }

    pub(crate) fn action_paste_image(&mut self) {
        if !self.guard_unsaved_edit() {
            return;
        }
        self.selection.deselect();
        match crate::clipboard::paste_image_from_clipboard(self.hwnd) {
            Ok(Some(image)) => self.open_pasted_image(&image),
            Ok(None) => {} // クリップボードに画像なし
            Err(e) => self.show_error_title(&format!("貼り付けに失敗しました: {e:#}")),
        }
    }

    /// 貼り付けた画像を一時ファイルへ保存してから開く
    pub(super) fn open_pasted_image(&mut self, image: &DecodedImage) {
        let path = match self.pasted_images.save(image) {
            Ok(path) => path,
            Err(e) => {
                self.show_error_title(&format!("貼り付けに失敗しました: {e:#}"));
                return;
            }
        };
        if let Err(e) = self.document.open_single(&path) {
            self.show_error_title(&format!("貼り付けに失敗しました: {e:#}"));
        }
        self.process_document_events();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_operation_directory_shares_successful_destinations_until_reset() {
        let source_dir = Path::new(r"C:\images\source");
        let selected_folder = Path::new(r"D:\organized");
        let saved_file = Path::new(r"E:\exports\image.png");
        let mut directory = FileOperationDirectory::default();

        assert_eq!(
            directory.initial(Some(source_dir)).as_deref(),
            Some(source_dir)
        );

        directory.remember_folder(selected_folder);
        assert_eq!(
            directory.initial(Some(source_dir)).as_deref(),
            Some(selected_folder)
        );

        directory.remember_file(saved_file);
        assert_eq!(
            directory.initial(Some(source_dir)).as_deref(),
            Some(Path::new(r"E:\exports"))
        );

        directory.reset();
        assert_eq!(
            directory.initial(Some(source_dir)).as_deref(),
            Some(source_dir)
        );
    }

    use crate::app::test_support::{TestApp, solid_image, unique_temp_dir};

    fn decode_file(path: &Path) -> DecodedImage {
        let rgba = image::open(path).unwrap().into_rgba8();
        let (width, height) = rgba.dimensions();
        DecodedImage {
            data: rgba.into_raw(),
            width,
            height,
        }
    }

    /// 貼り付け先を指定したフォルダへ向けたウィンドウ
    fn app_pasting_into(dir: &Path) -> TestApp {
        let mut app = TestApp::new();
        app.pasted_images = PastedImageFiles::new(dir.to_path_buf());
        app
    }

    fn assert_same_image(actual: &DecodedImage, expected: &DecodedImage) {
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.data, expected.data);
    }

    /// ウィンドウごとの貼り付け画像は、再読込・ファイル内容の取得・画像出力で他のウィンドウの画像に置き換わらない
    #[test]
    fn pasted_images_are_independent_between_windows() {
        let dir = unique_temp_dir("paste_windows");
        let image_a = solid_image(4, 3, [255, 0, 0, 255]);
        let image_b = solid_image(5, 2, [0, 0, 255, 255]);
        let mut app_a = app_pasting_into(&dir);
        let mut app_b = app_pasting_into(&dir);
        app_a.open_pasted_image(&image_a);
        app_b.open_pasted_image(&image_b);

        app_a.document.reload();
        app_a.process_document_events();
        assert_same_image(app_a.document.current_image().unwrap(), &image_a);
        let data = app_a.document.read_file_data_current().unwrap();
        let rgba = image::load_from_memory(&data).unwrap().into_rgba8();
        assert_eq!(rgba.as_raw(), &image_a.data);
        let out = dir.join("export.png");
        app_a
            .write_current_image(super::super::ExportFormat::Png, &out)
            .unwrap();
        assert_same_image(&decode_file(&out), &image_a);

        app_a.destroy();
        app_b.destroy();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同じウィンドウで繰り返し貼り付けると別のファイルを作成し、前の内容を変えない
    #[test]
    fn repeated_paste_creates_new_files() {
        let dir = unique_temp_dir("paste_repeat");
        let first = solid_image(2, 2, [10, 20, 30, 255]);
        let second = solid_image(3, 3, [200, 100, 0, 255]);
        let mut app = app_pasting_into(&dir);
        app.open_pasted_image(&first);
        app.open_pasted_image(&second);

        let files = app.pasted_images.files.clone();
        assert_eq!(files.len(), 2);
        assert_ne!(files[0], files[1]);
        assert_same_image(&decode_file(&files[0]), &first);
        assert_same_image(&decode_file(&files[1]), &second);
        assert_same_image(app.document.current_image().unwrap(), &second);

        app.destroy();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 保存に失敗した貼り付けは通知し、以前に貼り付けたファイルと表示中の画像を変えない
    #[test]
    fn paste_save_failure_is_reported_and_keeps_previous() {
        let dir = unique_temp_dir("paste_fail");
        let first = solid_image(2, 2, [1, 2, 3, 255]);
        let mut app = app_pasting_into(&dir);
        app.open_pasted_image(&first);
        let previous = app.pasted_images.files[0].clone();
        let previous_bytes = std::fs::read(&previous).unwrap();

        // 存在しないフォルダを保存先にして書き込みを失敗させる
        app.pasted_images.dir = dir.join("missing");
        app.begin_user_operation();
        app.open_pasted_image(&solid_image(2, 2, [9, 9, 9, 255]));
        let title = app.title();
        assert!(title.contains("貼り付けに失敗しました"), "{title}");
        assert_eq!(app.pasted_images.files, vec![previous.clone()]);
        assert_eq!(std::fs::read(&previous).unwrap(), previous_bytes);
        assert_same_image(app.document.current_image().unwrap(), &first);

        app.destroy();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 書き込み途中で失敗した貼り付けは、書き込み途中のファイルだけを削除する
    #[test]
    fn write_failure_removes_partial_file() {
        let dir = unique_temp_dir("paste_partial");
        let first = solid_image(2, 2, [5, 6, 7, 255]);
        let mut app = app_pasting_into(&dir);
        app.open_pasted_image(&first);
        let previous = app.pasted_images.files[0].clone();

        app.pasted_images.fail_next_write = true;
        app.begin_user_operation();
        app.open_pasted_image(&solid_image(2, 2, [9, 9, 9, 255]));
        assert!(
            app.title().contains("貼り付けに失敗しました"),
            "{}",
            app.title()
        );
        let remaining: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(remaining, vec![previous.clone()]);
        assert_eq!(app.pasted_images.files, vec![previous]);

        app.destroy();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ウィンドウを閉じると、そのウィンドウが作成した貼り付けファイルだけを削除する
    #[test]
    fn destroying_window_removes_only_own_pasted_files() {
        let dir = unique_temp_dir("paste_destroy");
        let image_b = solid_image(3, 2, [0, 255, 0, 255]);
        let mut app_a = app_pasting_into(&dir);
        let mut app_b = app_pasting_into(&dir);
        app_a.open_pasted_image(&solid_image(2, 2, [255, 255, 0, 255]));
        app_b.open_pasted_image(&image_b);
        let path_a = app_a.pasted_images.files[0].clone();
        let path_b = app_b.pasted_images.files[0].clone();

        app_a.destroy();
        assert!(!path_a.exists());
        assert!(path_b.exists());

        app_b.document.reload();
        app_b.process_document_events();
        assert_same_image(app_b.document.current_image().unwrap(), &image_b);
        assert!(!app_b.title().contains("エラー"), "{}", app_b.title());

        app_b.destroy();
        assert!(!path_b.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
