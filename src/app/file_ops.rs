//! ファイル操作アクション群
//!
//! 削除、移動、コピー、マーク操作、クリップボード貼り付けなど。
//! 各操作の結果は`AppWindow::take_success`で成功・キャンセル・失敗へ分け、失敗だけを通知する。

#[cfg(test)]
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

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
    /// 試験経路: 次の書き込みを失敗させる
    #[cfg(test)]
    fail_next_write: bool,
}

impl PastedImageFiles {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            files: Vec::new(),
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

        let (path, mut file) = crate::temp_cleanup::create_paste_file(&self.dir)
            .context("一時ファイルを作成できませんでした")?;
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

    /// このウィンドウが作成した一時ファイルだけを削除する (ウィンドウ破棄時の後始末)
    pub(super) fn remove_all(&mut self) {
        for path in self.files.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl AppWindow {
    /// Shellのファイル操作が奪ったフォーカスを元のウィンドウへ戻す。
    fn restore_file_operation_focus(&self) {
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
        });
    }

    pub(crate) fn action_delete_file(&mut self) {
        if !self
            .document
            .current_source()
            .is_some_and(crate::file_info::FileSource::can_delete)
        {
            return;
        }
        let Some(path) = self.document.current_path().map(Path::to_path_buf) else {
            return;
        };
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                crate::shell::file_operations::delete_to_recycle_bin(hwnd, &[&path])
                    .map(|done| done.then_some(()))
            },
            |app, result| {
                app.restore_file_operation_focus();
                if app.take_success("ファイルの削除", result).is_some() {
                    app.navigate_with_guard(crate::document::Document::remove_current_from_list);
                }
            },
        );
    }

    pub(crate) fn action_move_file(&mut self) {
        let Some(source) = self.document.current_source().cloned() else {
            return;
        };
        if !source.can_move() {
            return;
        }
        let Some(path) = source.file_path().map(Path::to_path_buf) else {
            return;
        };
        let initial_dir = self.file_operation_directory.initial(source.parent_dir());
        let default_name = source.default_save_name();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                crate::ui::file_dialog::save_file_dialog(
                    hwnd,
                    crate::ui::file_dialog::SaveFileDialogParams {
                        default_name: &default_name,
                        filter_name: "すべてのファイル",
                        filter_ext: "*.*",
                        initial_dir: initial_dir.as_deref(),
                        title: Some("ファイルを移動"),
                        ok_button_label: Some("移動"),
                        ..Default::default()
                    },
                )
                .map(|dest| {
                    dest.map(|dest| {
                        let result =
                            crate::shell::file_operations::move_single_file(hwnd, &path, &dest)
                                .map(|done| done.then_some(()));
                        (dest, result)
                    })
                })
            },
            |app, result| {
                if let Some((dest, result)) = app.take_success("保存ダイアログの表示", result)
                {
                    app.restore_file_operation_focus();
                    if app.take_success("ファイルの移動", result).is_some() {
                        app.file_operation_directory.remember_file(&dest);
                        app.navigate_with_guard(
                            crate::document::Document::remove_current_from_list,
                        );
                    }
                }
            },
        );
    }

    pub(crate) fn action_copy_file(&mut self) {
        let Some(source) = self.document.current_source().cloned() else {
            return;
        };
        if !source.can_copy() {
            return;
        }
        let initial_dir = self.file_operation_directory.initial(source.parent_dir());
        let default_name = source.default_save_name();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                crate::ui::file_dialog::save_file_dialog(
                    hwnd,
                    crate::ui::file_dialog::SaveFileDialogParams {
                        default_name: &default_name,
                        filter_name: "すべてのファイル",
                        filter_ext: "*.*",
                        initial_dir: initial_dir.as_deref(),
                        title: Some("ファイルを複製"),
                        ok_button_label: Some("複製"),
                        ..Default::default()
                    },
                )
            },
            move |app, result| {
                if let Some(dest) = app.take_success("保存ダイアログの表示", result) {
                    let result = app.document.copy_source_to(&source, &dest).map(Some);
                    if app.take_success("ファイルの複製", result).is_some() {
                        app.file_operation_directory.remember_file(&dest);
                    }
                }
            },
        );
    }

    /// マークごとの操作能力を判定する。表示中の行の種別には依存しない。
    fn marked_targets(&self, copy: bool) -> (Vec<usize>, usize) {
        let marked = self.document.file_list().marked_indices();
        let targets: Vec<_> = marked
            .iter()
            .copied()
            .filter(|&index| {
                let source = &self.document.file_list().files()[index].source;
                if copy {
                    source.can_copy()
                } else {
                    source.can_move()
                }
            })
            .collect();
        let skipped = marked.len() - targets.len();
        (targets, skipped)
    }

    fn notify_skipped(&mut self, skipped: usize, copy: bool) {
        if skipped > 0 {
            let reason = if copy {
                "未展開コンテナ"
            } else {
                "アーカイブ内画像・PDFページ・未展開コンテナ"
            };
            let notice = format!("対象外 {skipped} 件: {reason}");
            let notice = self
                .error_message
                .as_ref()
                .map_or_else(|| notice.clone(), |error| format!("{error} / {notice}"));
            self.show_error_title(&notice);
        }
    }

    pub(crate) fn action_marked_delete(&mut self) {
        let (targets, skipped) = self.marked_targets(false);
        if targets.is_empty() {
            self.notify_skipped(skipped, false);
        } else {
            let paths: Vec<_> = targets
                .iter()
                .map(|&index| {
                    self.document.file_list().files()[index]
                        .source
                        .file_path()
                        .expect("操作対象の通常ファイル")
                        .to_path_buf()
                })
                .collect();
            let hwnd = self.hwnd;
            self.defer_modal(
                move || {
                    let refs: Vec<_> = paths.iter().map(PathBuf::as_path).collect();
                    crate::shell::file_operations::delete_to_recycle_bin(hwnd, &refs)
                        .map(|done| done.then_some(()))
                },
                move |app, result| {
                    app.restore_file_operation_focus();
                    if app.take_success("マークファイルの削除", result).is_some() {
                        app.navigate_with_guard(|document| {
                            document.remove_indices_from_list(&targets);
                        });
                    }
                    app.notify_skipped(skipped, false);
                },
            );
        }
    }

    pub(crate) fn action_marked_move(&mut self) {
        self.marked_transfer(false);
    }
    pub(crate) fn action_marked_copy(&mut self) {
        self.marked_transfer(true);
    }

    fn marked_transfer(&mut self, copy: bool) {
        let (targets, skipped) = self.marked_targets(copy);
        if targets.is_empty() {
            self.notify_skipped(skipped, copy);
            return;
        }
        let sources: Vec<_> = targets
            .iter()
            .map(|&index| self.document.file_list().files()[index].source.clone())
            .collect();
        let initial_dir = self
            .file_operation_directory
            .initial(sources[0].parent_dir());
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                let result = crate::ui::file_dialog::select_folder_dialog(
                    hwnd,
                    if copy {
                        "複製先フォルダ"
                    } else {
                        "移動先フォルダ"
                    },
                    initial_dir.as_deref(),
                )
                .map(|dest| {
                    dest.map(|dest| {
                        let move_result = if copy {
                            None
                        } else {
                            let paths: Vec<_> = sources
                                .iter()
                                .map(|source| source.file_path().expect("操作対象の通常ファイル"))
                                .collect();
                            Some(
                                crate::shell::file_operations::move_files(hwnd, &paths, &dest)
                                    .map(|done| done.then_some(())),
                            )
                        };
                        (dest, move_result)
                    })
                });
                (sources, result)
            },
            move |app, (sources, result)| {
                if let Some((dest, move_result)) =
                    app.take_success("フォルダ選択ダイアログの表示", result)
                {
                    if copy {
                        let mut copied = false;
                        for source in &sources {
                            let path = crate::archive::resolve_filename(
                                &dest,
                                &source.default_save_name(),
                            );
                            let result = app.document.copy_source_to(source, &path).map(Some);
                            copied |= app.take_success("マークファイルの複製", result).is_some();
                        }
                        if copied {
                            app.file_operation_directory.remember_folder(&dest);
                        }
                    } else {
                        let result = move_result.expect("移動の要求はShell操作の結果を持つ");
                        app.restore_file_operation_focus();
                        if app.take_success("マークファイルの移動", result).is_some() {
                            app.file_operation_directory.remember_folder(&dest);
                            app.navigate_with_guard(|document| {
                                document.remove_indices_from_list(&targets);
                            });
                        }
                    }
                }
                app.notify_skipped(skipped, copy);
            },
        );
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
            let hwnd = self.hwnd;
            self.defer_call(
                move || crate::clipboard::copy_text_to_clipboard(hwnd, &text),
                |app, result| {
                    if let Err(e) = result {
                        app.show_error_title(&format!(
                            "マークファイル名のコピーに失敗しました: {e:#}"
                        ));
                    }
                },
            );
        }
    }

    pub(crate) fn action_paste_image(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        let hwnd = self.hwnd;
        self.defer_call(
            move || crate::clipboard::paste_image_from_clipboard(hwnd),
            |app, result| match result {
                Ok(Some(image)) => app.open_pasted_image(&image),
                Ok(None) => {} // クリップボードに画像なし
                Err(e) => app.show_error_title(&format!("貼り付けに失敗しました: {e:#}")),
            },
        );
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

impl AppWindow {
    pub(super) fn action_copy_file_name(&mut self) {
        if let Some(source) = self.document.current_source() {
            let text = source.display_path();
            let hwnd = self.hwnd;
            self.defer_call(
                move || crate::clipboard::copy_text_to_clipboard(hwnd, &text),
                |app, result| {
                    if let Err(e) = result {
                        app.show_error_title(&format!("ファイル名のコピーに失敗しました: {e:#}"));
                    }
                },
            );
        }
    }

    pub(super) fn action_copy_image(&mut self) {
        if let Some(image) = self.document.current_image() {
            let target =
                crate::filter::transform::output_image(image, self.selection.current_rect())
                    .into_owned();
            let hwnd = self.hwnd;
            self.defer_call(
                move || crate::clipboard::copy_image_to_clipboard(hwnd, &target),
                |app, result| {
                    if let Err(e) = result {
                        app.show_error_title(&format!("画像のコピーに失敗しました: {e:#}"));
                    }
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{assert_same_image, decode_file};
    use std::io::Write;

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

    use crate::app::test_support::TestApp;
    use crate::test_helpers::{TempDir, solid_image};

    /// 貼り付け先を指定したフォルダへ向けたウィンドウ
    fn app_pasting_into(dir: &Path) -> TestApp {
        let app = TestApp::new();
        app.with_app(|state| state.pasted_images = PastedImageFiles::new(dir.to_path_buf()));
        app
    }

    /// ウィンドウごとの貼り付け画像は、再読込・ファイル内容の取得・画像出力で他のウィンドウの画像に置き換わらない
    #[test]
    fn pasted_images_are_independent_between_windows() {
        let dir = TempDir::new("paste_windows");
        let image_a = solid_image(4, 3, [255, 0, 0, 255]);
        let image_b = solid_image(5, 2, [0, 0, 255, 255]);
        let app_a = app_pasting_into(&dir);
        let app_b = app_pasting_into(&dir);
        app_a.with_app(|state| state.open_pasted_image(&image_a));
        app_b.with_app(|state| state.open_pasted_image(&image_b));

        app_a.with_app(|state| state.document.reload());
        app_a.with_app(super::super::AppWindow::process_document_events);
        assert_same_image(app_a.borrow().document.current_image().unwrap(), &image_a);
        let data = app_a.borrow().document.read_file_data_current().unwrap();
        let rgba = image::load_from_memory(&data).unwrap().into_rgba8();
        assert_eq!(rgba.as_raw(), &image_a.data);
        let out = dir.join("export.png");
        app_a
            .with_app(|state| state.write_current_image(super::super::ExportFormat::Png, &out))
            .unwrap();
        assert_same_image(&decode_file(&out), &image_a);

        app_a.destroy();
        app_b.destroy();
    }

    /// 同じウィンドウで繰り返し貼り付けると別のファイルを作成し、前の内容を変えない
    #[test]
    fn repeated_paste_creates_new_files() {
        let dir = TempDir::new("paste_repeat");
        let first = solid_image(2, 2, [10, 20, 30, 255]);
        let second = solid_image(3, 3, [200, 100, 0, 255]);
        let app = app_pasting_into(&dir);
        app.with_app(|state| state.open_pasted_image(&first));
        app.with_app(|state| state.open_pasted_image(&second));

        let files = app.borrow().pasted_images.files.clone();
        assert_eq!(files.len(), 2);
        assert_ne!(files[0], files[1]);
        assert_same_image(&decode_file(&files[0]), &first);
        assert_same_image(&decode_file(&files[1]), &second);
        assert_same_image(app.borrow().document.current_image().unwrap(), &second);

        app.destroy();
    }

    /// 保存に失敗した貼り付けは通知し、以前に貼り付けたファイルと表示中の画像を変えない
    #[test]
    fn paste_save_failure_is_reported_and_keeps_previous() {
        let dir = TempDir::new("paste_fail");
        let first = solid_image(2, 2, [1, 2, 3, 255]);
        let app = app_pasting_into(&dir);
        app.with_app(|state| state.open_pasted_image(&first));
        let previous = app.borrow().pasted_images.files[0].clone();
        let previous_bytes = std::fs::read(&previous).unwrap();

        // 存在しないフォルダを保存先にして書き込みを失敗させる
        app.with_app(|state| state.pasted_images.dir = dir.join("missing"));
        app.with_app(super::super::AppWindow::begin_user_operation);
        app.with_app(|state| state.open_pasted_image(&solid_image(2, 2, [9, 9, 9, 255])));
        let title = app.title();
        assert!(title.contains("貼り付けに失敗しました"), "{title}");
        assert_eq!(app.borrow().pasted_images.files, vec![previous.clone()]);
        assert_eq!(std::fs::read(&previous).unwrap(), previous_bytes);
        assert_same_image(app.borrow().document.current_image().unwrap(), &first);

        app.destroy();
    }

    /// 書き込み途中で失敗した貼り付けは、書き込み途中のファイルだけを削除する
    #[test]
    fn write_failure_removes_partial_file() {
        let dir = TempDir::new("paste_partial");
        let first = solid_image(2, 2, [5, 6, 7, 255]);
        let app = app_pasting_into(&dir);
        app.with_app(|state| state.open_pasted_image(&first));
        let previous = app.borrow().pasted_images.files[0].clone();

        app.with_app(|state| state.pasted_images.fail_next_write = true);
        app.with_app(super::super::AppWindow::begin_user_operation);
        app.with_app(|state| state.open_pasted_image(&solid_image(2, 2, [9, 9, 9, 255])));
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
        assert_eq!(app.borrow().pasted_images.files, vec![previous]);

        app.destroy();
    }

    /// ウィンドウを閉じると、そのウィンドウが作成した貼り付けファイルだけを削除する
    #[test]
    fn destroying_window_removes_only_own_pasted_files() {
        let dir = TempDir::new("paste_destroy");
        let image_b = solid_image(3, 2, [0, 255, 0, 255]);
        let app_a = app_pasting_into(&dir);
        let app_b = app_pasting_into(&dir);
        app_a.with_app(|state| state.open_pasted_image(&solid_image(2, 2, [255, 255, 0, 255])));
        app_b.with_app(|state| state.open_pasted_image(&image_b));
        let path_a = app_a.borrow().pasted_images.files[0].clone();
        let path_b = app_b.borrow().pasted_images.files[0].clone();

        app_a.destroy();
        assert!(!path_a.exists());
        assert!(path_b.exists());

        app_b.with_app(|state| state.document.reload());
        app_b.with_app(super::super::AppWindow::process_document_events);
        assert_same_image(app_b.borrow().document.current_image().unwrap(), &image_b);
        assert!(!app_b.title().contains("エラー"), "{}", app_b.title());

        app_b.destroy();
        assert!(!path_b.exists());
    }
    /// 通常画像・ZIP・展開済み画像・PDFを同時に持つ受入用ウィンドウ。
    fn mixed_source_app(dir: &Path) -> (TestApp, PathBuf, Vec<crate::file_info::FileSource>) {
        use crate::file_info::{FileInfo, FileSource};
        let dir = crate::util::strip_extended_length_prefix(&std::fs::canonicalize(dir).unwrap());
        let png = crate::test_helpers::create_1x1_white_png();
        let image = dir.join("image.png");
        std::fs::write(&image, &png).unwrap();
        let archive = dir.join("archive.zip");
        let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file("inside.png", ::zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&png).unwrap();
        writer.finish().unwrap();
        let pdf = dir.join("document.pdf");
        std::fs::write(&pdf, crate::test_helpers::minimal_pdf()).unwrap();
        let extracted = dir.join("extracted.png");
        std::fs::write(&extracted, &png).unwrap();
        let mut info = FileInfo::from_path(&extracted).unwrap();
        info.source = FileSource::ArchiveEntry {
            archive: dir.join("original.7z"),
            entry: "folder/extracted.png".into(),
            on_demand: false,
            temp_path: Some(extracted),
            entry_index: None,
        };
        let app = TestApp::new();
        app.with_app(|state| state.document.open_multiple(&[image.clone(), archive, pdf]))
            .unwrap();
        app.with_app(|state| state.document.expand_all_pending_sync());
        app.with_app(|state| state.document.append_test_file(info));
        assert_eq!(app.borrow().document.file_list().len(), 4);
        for index in 0..4 {
            app.with_app(|state| state.document.navigate_to(index));
            app.with_app(|state| state.document.mark_current());
        }
        app.with_app(super::super::AppWindow::process_document_events);
        let sources = app
            .borrow()
            .document
            .file_list()
            .files()
            .iter()
            .map(|file| file.source.clone())
            .collect();
        (app, image, sources)
    }

    #[test]
    fn marked_targets_depend_on_each_source_and_keep_excluded_marks() {
        use crate::file_info::FileSource;
        let dir = TempDir::new("marked_mixed");
        let (app, image, sources) = mixed_source_app(&dir);
        let current_indices: Vec<_> = sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                matches!(
                    source,
                    FileSource::File(_)
                        | FileSource::PdfPage { .. }
                        | FileSource::ArchiveEntry {
                            on_demand: true,
                            ..
                        }
                )
                .then_some(index)
            })
            .collect();
        assert_eq!(current_indices.len(), 3);
        for index in current_indices {
            app.with_app(|state| state.document.navigate_to(index));
            app.with_app(super::super::AppWindow::process_document_events);
            let (targets, skipped) = app.with_app(|state| state.marked_targets(false));
            assert_eq!((targets.len(), skipped), (1, 3));
            assert_eq!(sources[targets[0]].file_path(), Some(image.as_path()));
            assert_eq!(
                app.with_app(|state| state.marked_targets(true)),
                ((0..4).collect(), 0)
            );
            for moving in [false, true] {
                app.with_app(super::super::AppWindow::begin_user_operation);
                let captured = crate::shell::file_operations::test_driver::with_selected_path(
                    dir.path().to_path_buf(),
                    || {
                        if moving {
                            app.with_app(super::super::AppWindow::action_marked_move);
                        } else {
                            app.with_app(super::super::AppWindow::action_marked_delete);
                        }
                    },
                );
                assert_eq!(
                    captured,
                    vec![(if moving { "move" } else { "delete" }, vec![image.clone()])]
                );
                let title = app.title();
                assert!(title.contains("対象外 3 件"), "{title}");
                assert!(
                    title.contains("アーカイブ内画像・PDFページ・未展開コンテナ"),
                    "{title}"
                );
                // Shell境界のキャンセルでは対象行とマークを残す。
                assert_eq!(app.borrow().document.file_list().len(), 4);
                assert_eq!(app.borrow().document.file_list().marked_count(), 4);
            }
        }
        let (targets, _) = app.with_app(|state| state.marked_targets(false));
        app.with_app(|state| state.document.remove_indices_from_list(&targets));
        app.with_app(super::super::AppWindow::process_document_events);
        assert_eq!(app.borrow().document.file_list().len(), 3);
        assert_eq!(app.borrow().document.file_list().marked_count(), 3);
        app.close_without_release();
    }

    /// 全表示元でロックされたコピー先と元データを保持し、解除後は同じ操作で保存する。
    #[test]
    fn single_copy_failure_keeps_existing_content_for_each_source_and_can_retry() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let dir = TempDir::new("mixed_copy_retry");
        let (app, _, sources) = mixed_source_app(&dir);
        let output_dir = dir.join("copies");
        std::fs::create_dir(&output_dir).unwrap();
        let output = output_dir.join("destination.dat");
        for index in 0..sources.len() {
            app.with_app(|state| state.document.navigate_to(index));
            app.with_app(super::super::AppWindow::process_document_events);
            let expected = app.borrow().document.read_file_data_current().unwrap();
            std::fs::write(&output, b"original").unwrap();
            let locked = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&output)
                .unwrap();
            app.with_app(super::super::AppWindow::begin_user_operation);
            crate::shell::file_operations::test_driver::with_selected_path(output.clone(), || {
                app.with_app(super::super::AppWindow::action_copy_file);
            });
            assert!(
                app.title().contains("ファイルの複製に失敗しました"),
                "{}",
                app.title()
            );
            drop(locked);
            assert_eq!(std::fs::read(&output).unwrap(), b"original");
            assert_eq!(
                app.borrow().document.read_file_data_current().unwrap(),
                expected
            );
            assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 1);

            app.with_app(super::super::AppWindow::begin_user_operation);
            crate::shell::file_operations::test_driver::with_selected_path(output.clone(), || {
                app.with_app(super::super::AppWindow::action_copy_file);
            });
            assert_eq!(std::fs::read(&output).unwrap(), expected);
            assert!(!app.title().contains("失敗"), "{}", app.title());
            assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 1);
        }
        app.close_without_release();
    }

    #[test]
    fn marked_copy_saves_each_source_content_and_pdf_single_copy_is_png() {
        use crate::file_info::FileSource;
        let dir = TempDir::new("mixed_copy");
        let (app, image, sources) = mixed_source_app(&dir);
        let normal_index = sources
            .iter()
            .position(|source| matches!(source, FileSource::File(_)))
            .unwrap();
        app.with_app(|state| state.document.navigate_to(normal_index));
        app.with_app(super::super::AppWindow::process_document_events);
        let destination = dir.join("copies");
        std::fs::create_dir(&destination).unwrap();
        app.with_app(super::super::AppWindow::begin_user_operation);
        let captured = crate::shell::file_operations::test_driver::with_selected_path(
            destination.clone(),
            || app.with_app(super::super::AppWindow::action_marked_copy),
        );
        assert!(captured.is_empty(), "コンテナ本体をShellへ渡さない");
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 4);
        for source in &sources {
            let bytes = std::fs::read(destination.join(source.default_save_name())).unwrap();
            if matches!(source, FileSource::PdfPage { .. }) {
                assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
                let png = image::load_from_memory(&bytes).unwrap();
                assert_eq!((png.width(), png.height()), (6, 3));
            } else {
                assert_eq!(bytes, std::fs::read(&image).unwrap());
            }
        }
        assert!(!destination.join("archive.zip").exists());
        assert!(!destination.join("document.pdf").exists());
        assert!(!app.title().contains("失敗"), "{}", app.title());
        let pdf_index = sources
            .iter()
            .position(|source| matches!(source, FileSource::PdfPage { .. }))
            .unwrap();
        app.with_app(|state| state.document.navigate_to(pdf_index));
        app.with_app(super::super::AppWindow::process_document_events);
        let single_dir = dir.join("single");
        std::fs::create_dir(&single_dir).unwrap();
        let output = single_dir.join(sources[pdf_index].default_save_name());
        app.with_app(super::super::AppWindow::begin_user_operation);
        let captured =
            crate::shell::file_operations::test_driver::with_selected_path(output.clone(), || {
                app.with_app(super::super::AppWindow::action_copy_file);
            });
        assert!(captured.is_empty());
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let png = image::load_from_memory(&bytes).unwrap();
        assert_eq!((png.width(), png.height()), (6, 3));
        assert_eq!(std::fs::read_dir(&single_dir).unwrap().count(), 1);
        assert!(!single_dir.join("document.pdf").exists());
        assert!(!app.title().contains("失敗"), "{}", app.title());
        app.close_without_release();
    }
}

#[cfg(test)]
mod move_action_acceptance {
    use super::AppWindow;
    use crate::app::test_support::TestApp;
    use crate::file_info::{FileInfo, FileSource};
    use crate::shell::file_operations::test_driver::with_selected_path_and_real_moves;
    use crate::test_helpers::{TempDir, solid_image, write_png};
    use std::path::{Path, PathBuf};

    fn app_with_move_images(dir: &Path) -> (TestApp, [PathBuf; 3]) {
        let dir = crate::util::strip_extended_length_prefix(&std::fs::canonicalize(dir).unwrap());
        let source = dir.join("source");
        std::fs::create_dir(&source).unwrap();
        let paths = ["a.png", "b.png", "c.png"].map(|name| source.join(name));
        for (path, red) in paths.iter().zip([10, 11, 12]) {
            write_png(path, &solid_image(2, 2, [red, 20, 30, 255]));
        }
        let app = TestApp::new();
        app.with_app(|state| state.document.open_folder(&source))
            .unwrap();
        app.with_app(AppWindow::process_document_events);
        assert_eq!(app.borrow().document.file_list().len(), 3);
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                app.borrow().document.file_list().files()[index]
                    .source
                    .file_path(),
                Some(path.as_path())
            );
        }
        (app, paths)
    }

    #[test]
    fn single_move_action_removes_moved_row_and_keeps_other_marks() {
        let _com = crate::test_helpers::StaComGuard::init().unwrap();
        let dir = TempDir::new("single_move_action");
        let (app, paths) = app_with_move_images(&dir);
        let expected: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let destination_dir = dir.join("destination");
        std::fs::create_dir(&destination_dir).unwrap();
        let destination = destination_dir.join("renamed.png");
        app.with_app(|state| {
            state.document.mark_current();
            state.document.navigate_to(1);
            state.document.mark_current();
            state.document.navigate_to(0);
        });
        app.with_app(AppWindow::begin_user_operation);
        let captured = with_selected_path_and_real_moves(destination.clone(), || {
            app.with_app(AppWindow::action_move_file);
        });
        assert_eq!(captured, vec![("move_single", vec![paths[0].clone()])]);
        assert!(!paths[0].exists());
        assert_eq!(std::fs::read(&destination).unwrap(), expected[0]);
        assert_eq!(std::fs::read(&paths[1]).unwrap(), expected[1]);
        assert_eq!(std::fs::read(&paths[2]).unwrap(), expected[2]);
        assert_eq!(std::fs::read_dir(&destination_dir).unwrap().count(), 1);
        {
            let state = app.borrow();
            let files = state.document.file_list().files();
            assert_eq!(files.len(), 2);
            assert_eq!(state.document.file_list().marked_count(), 1);
            assert_eq!(files[0].source.file_path(), Some(paths[1].as_path()));
            assert!(files[0].marked);
            assert_eq!(files[1].source.file_path(), Some(paths[2].as_path()));
            assert!(!files[1].marked);
            assert!(
                files
                    .iter()
                    .all(|file| file.source.file_path() != Some(paths[0].as_path()))
            );
        }
        assert!(!app.title().contains("失敗"), "{}", app.title());
        app.destroy();
    }

    #[test]
    fn marked_move_action_moves_supported_marks_and_keeps_excluded_mark() {
        let _com = crate::test_helpers::StaComGuard::init().unwrap();
        let dir = TempDir::new("marked_move_action");
        let (app, paths) = app_with_move_images(&dir);
        let expected: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let destination = dir.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let excluded = dir.join("excluded.png");
        write_png(&excluded, &solid_image(3, 2, [80, 90, 100, 255]));
        let excluded_bytes = std::fs::read(&excluded).unwrap();
        let mut info = FileInfo::from_path(&excluded).unwrap();
        info.source = FileSource::ArchiveEntry {
            archive: dir.join("original.7z"),
            entry: "excluded.png".into(),
            on_demand: false,
            temp_path: Some(excluded.clone()),
            entry_index: None,
        };
        info.marked = true;
        app.with_app(|state| {
            state.document.append_test_file(info);
            state.document.mark_current();
            state.document.navigate_to(1);
            state.document.mark_current();
            state.document.navigate_to(0);
        });
        assert_eq!(app.borrow().document.file_list().marked_count(), 3);
        app.with_app(AppWindow::begin_user_operation);
        let captured = with_selected_path_and_real_moves(destination.clone(), || {
            app.with_app(AppWindow::action_marked_move);
        });
        assert_eq!(
            captured,
            vec![("move", vec![paths[0].clone(), paths[1].clone()])]
        );
        for (path, bytes) in paths[..2].iter().zip(&expected[..2]) {
            assert!(!path.exists());
            assert_eq!(
                std::fs::read(destination.join(path.file_name().unwrap())).unwrap(),
                *bytes
            );
        }
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 2);
        assert_eq!(std::fs::read(&paths[2]).unwrap(), expected[2]);
        assert_eq!(std::fs::read(&excluded).unwrap(), excluded_bytes);
        {
            let state = app.borrow();
            let files = state.document.file_list().files();
            assert_eq!(files.len(), 2);
            assert_eq!(state.document.file_list().marked_count(), 1);
            assert_eq!(files[0].source.file_path(), Some(paths[2].as_path()));
            assert!(!files[0].marked);
            assert!(matches!(
                &files[1].source,
                FileSource::ArchiveEntry { temp_path: Some(path), .. } if path == &excluded
            ));
            assert!(files[1].marked);
            for moved in &paths[..2] {
                assert!(
                    files
                        .iter()
                        .all(|file| file.source.file_path() != Some(moved.as_path()))
                );
            }
        }
        assert!(app.title().contains("対象外 1 件"), "{}", app.title());
        assert!(!app.title().contains("失敗"), "{}", app.title());
        app.destroy();
    }
}
