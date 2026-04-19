//! ナビゲーション操作

use super::AppWindow;
use crate::document::Document;

impl AppWindow {
    /// 未保存確認 → 選択解除 → ナビゲーション操作 → イベント処理の共通パターン
    pub(crate) fn navigate_with_guard(&mut self, f: impl FnOnce(&mut Document)) {
        if !self.guard_unsaved_edit() {
            return;
        }
        self.selection.deselect();
        f(&mut self.document);
        self.process_document_events();
    }

    /// ページ指定ナビゲーション
    pub(crate) fn navigate_to_page_dialog(&mut self) {
        let total = self.document.file_list().len();
        if total == 0 {
            return;
        }
        let current = self.document.file_list().current_index().unwrap_or(0) + 1;
        if let Some(page) = crate::ui::page_dialog::show_page_dialog(self.hwnd, current, total) {
            let index = (page.saturating_sub(1)).min(total - 1);
            self.stop_slideshow();
            self.document.navigate_to(index);
            self.process_document_events();
        }
    }
}
