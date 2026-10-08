//! ブックマークの保存と読み込み

use super::AppWindow;

impl AppWindow {
    pub(super) fn action_bookmark_load(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::file_dialog::open_bookmark_dialog(hwnd),
            |app, result| {
                let Some(path) = app.take_success("ブックマークの読み込み", result)
                else {
                    return;
                };
                // 旧形式の拡張子判定と読み込みは、ダイアログを閉じた後の短い借用で行う。
                let result = crate::bookmark::load_bookmark_from_path(&path, &|p| {
                    app.document.is_archive_path(p)
                })
                .map(Some);
                let Some(data) = app.take_success("ブックマークの読み込み", result)
                else {
                    return;
                };
                let result = app.document.load_bookmark_data(data).map(Some);
                if app.take_success("ブックマークの読み込み", result).is_some() {
                    app.file_operation_directory.reset();
                    // 読み込み成功時のみ前回名キャッシュを更新する。
                    // 反映後のファイルリスト先頭からコンテナ識別キーを取得し、
                    // 選択パスのファイル名部分とペアで保持する。
                    if let (Some(key), Some(file_name)) = (
                        app.document
                            .file_list()
                            .files()
                            .first()
                            .and_then(|f| f.source.bookmark_container_key()),
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .map(str::to_string),
                    ) {
                        app.last_bookmark = Some((key, file_name));
                    }
                }
                app.process_document_events();
            },
        );
    }

    pub(super) fn action_bookmark_save(&mut self) {
        // 未展開コンテナがあれば全て同期展開 (ブックマークは完全な状態で保存する)
        if self.document.file_list().has_pending() {
            self.document.expand_all_pending_sync();
            self.process_document_events();
        }
        let idx = self.document.file_list().current_index();
        let first_source = self
            .document
            .file_list()
            .files()
            .first()
            .map(|f| f.source.clone());
        // 現在のコンテナ識別キーが前回キャッシュと一致する場合のみ前回ファイル名を流用する。
        // 不一致 (別コンテナへ切り替えた直後など) では `None` を渡し、
        // ヘルパー側で代表ステムベースの初期名に戻す。
        let current_key = first_source
            .as_ref()
            .and_then(crate::file_info::FileSource::bookmark_container_key);
        let previous_name = self
            .last_bookmark
            .as_ref()
            .and_then(|(key, name)| (current_key.as_ref() == Some(key)).then_some(name.as_str()));
        let initial_name =
            crate::bookmark::build_initial_save_name(previous_name, first_source.as_ref());
        let entries: Vec<_> = self
            .document
            .file_list()
            .files()
            .iter()
            .map(|file| file.source.clone())
            .collect();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::bookmark::save_bookmark(hwnd, &entries, idx, &initial_name),
            move |app, result| {
                if let Some(saved_path) = app.take_success("ブックマークの保存", result) {
                    // 保存成功時のみキャッシュを更新する。コンテナ識別キーは現在の先頭ソースから取得する。
                    if let (Some(key), Some(file_name)) = (
                        current_key,
                        saved_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(str::to_string),
                    ) {
                        app.last_bookmark = Some((key, file_name));
                    }
                }
            },
        );
    }
}
