//! ナビゲーション操作

use super::AppWindow;
use crate::document::Document;

impl AppWindow {
    /// 未保存確認 → 選択範囲の引き継ぎ → ナビゲーション操作 → イベント処理の共通パターン
    pub(crate) fn navigate_with_guard(&mut self, f: impl FnOnce(&mut Document)) {
        if !self.guard_unsaved_edit() {
            return;
        }
        self.carry_over_selection();
        f(&mut self.document);
        self.process_document_events();
    }

    /// 画像切替をまたいで選択範囲を引き継ぐ
    ///
    /// 複数の画像に対して同一座標の領域を繰り返し指定できるよう、確定済みの選択矩形は切替後も保持する。
    /// 座標は切替先の画像の寸法に合わせて補正せず、各処理が境界クランプで扱う。
    /// ドラッグ操作の途中で切り替わった場合は、未確定の矩形が残らないよう解除する。
    pub(crate) fn carry_over_selection(&mut self) {
        if !self.selection.is_selected() {
            self.selection.deselect();
        }
    }

    /// ページ指定ナビゲーション
    pub(crate) fn navigate_to_page_dialog(&mut self) {
        let total = self.document.file_list().len();
        if total == 0 {
            return;
        }
        let current = self.document.file_list().current_index().unwrap_or(0) + 1;
        self.prepare_modal_dialog();
        let dialog_result = crate::ui::page_dialog::show_page_dialog(self.hwnd, current, total);
        self.finish_modal_dialog();
        if let Some(page) = dialog_result {
            let index = (page.saturating_sub(1)).min(total - 1);
            self.stop_slideshow();
            self.document.navigate_to(index);
            self.process_document_events();
        }
    }
}
