use super::{AppWindow, next_action};

impl AppWindow {
    pub(super) fn action_bookmark_load(&mut self) {
        self.before_image_change();
        self.selection.deselect();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::file_dialog::open_bookmark_dialog(hwnd),
            |app, result| {
                let Some(path) =
                    app.take_success("ブックマークの読み込み", next_action::BOOKMARK_LOAD, result)
                else {
                    return;
                };
                let result = crate::bookmark::load_bookmark_from_path(&path, &|p| {
                    app.document.is_archive_path(p)
                })
                .map(Some);
                let Some(data) =
                    app.take_success("ブックマークの読み込み", next_action::BOOKMARK_LOAD, result)
                else {
                    return;
                };
                let result = app.document.load_bookmark_data(data).map(Some);
                if app
                    .take_success("ブックマークの読み込み", next_action::BOOKMARK_LOAD, result)
                    .is_some()
                {
                    app.file_operation_directory.reset();
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
        let snapshot = self.document.bookmark_snapshot();
        if snapshot.expanded_pending {
            self.process_document_events();
        }
        let first_source = snapshot.entries.first();
        let current_key =
            first_source.and_then(crate::file_info::FileSource::bookmark_container_key);
        let previous_name = self
            .last_bookmark
            .as_ref()
            .and_then(|(key, name)| (current_key.as_ref() == Some(key)).then_some(name.as_str()));
        let initial_name = crate::bookmark::build_initial_save_name(previous_name, first_source);
        let hwnd = self.hwnd;
        self.defer_modal(
            move || {
                crate::bookmark::save_bookmark(
                    hwnd,
                    &snapshot.entries,
                    snapshot.index,
                    &initial_name,
                )
            },
            move |app, result| {
                if let Some(saved_path) =
                    app.take_success("ブックマークの保存", next_action::BOOKMARK_SAVE, result)
                    && let (Some(key), Some(file_name)) = (
                        current_key,
                        saved_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(str::to_string),
                    )
                {
                    app.last_bookmark = Some((key, file_name));
                }
            },
        );
    }
}
