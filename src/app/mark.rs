//! マークと一覧からの除外操作

use super::AppWindow;

impl AppWindow {
    pub(super) fn action_mark_set(&mut self) {
        self.document.mark_current();
        self.update_title();
        if self.file_list_panel.is_visible()
            && let Some(idx) = self.document.file_list().current_index()
        {
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.update_item(idx));
        }
    }

    pub(super) fn action_mark_unset(&mut self) {
        self.document.unmark_current();
        self.update_title();
        if self.file_list_panel.is_visible()
            && let Some(idx) = self.document.file_list().current_index()
        {
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.update_item(idx));
        }
    }

    pub(super) fn action_mark_invert_all(&mut self) {
        self.document.invert_all_marks();
        self.update_title();
        self.sync_file_list_panel();
    }

    pub(super) fn action_mark_invert_to_here(&mut self) {
        self.document.invert_marks_to_here();
        self.update_title();
        self.sync_file_list_panel();
    }

    pub(super) fn action_marked_remove_from_list(&mut self) {
        self.navigate_with_guard(crate::document::Document::remove_marked_from_list);
    }

    pub(super) fn action_remove_from_list(&mut self) {
        self.navigate_with_guard(crate::document::Document::remove_current_from_list);
    }
}
