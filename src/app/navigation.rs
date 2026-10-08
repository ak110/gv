//! ナビゲーション操作

use super::AppWindow;
use crate::document::Document;

impl AppWindow {
    pub(super) fn action_sort_navigate_forward(&mut self) {
        self.navigate_with_guard(Document::sort_navigate_forward);
    }

    /// 編集状態と選択範囲を整理して画像を切り替える
    pub(crate) fn navigate_with_guard(&mut self, f: impl FnOnce(&mut Document)) {
        self.before_image_change();
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
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::page_dialog::show_page_dialog(hwnd, current, total),
            move |app, result| {
                if let Some(page) = app.take_success("ダイアログの表示", result) {
                    let index = (page.saturating_sub(1)).min(total - 1);
                    app.stop_slideshow();
                    app.navigate_with_guard(|document| document.navigate_to(index));
                }
            },
        );
    }
}

impl AppWindow {
    /// 編集済みの画像を離れるときは、その画像上の選択範囲を解除する
    pub(super) fn before_image_change(&mut self) {
        if self.document.has_unsaved_edit() {
            self.selection.deselect();
        }
    }

    pub(super) fn action_shuffle_groups(&mut self) {
        self.navigate_with_guard(Document::shuffle_groups);
    }

    pub(super) fn action_shuffle_all(&mut self) {
        self.navigate_with_guard(Document::shuffle_all);
    }

    pub(super) fn action_navigate_to_page(&mut self) {
        self.navigate_to_page_dialog();
    }
}

#[cfg(test)]
mod tests {
    use crate::action::Action;
    use crate::app::test_support::TestApp;
    use crate::test_helpers::{TempDir, solid_image, write_png};
    use crate::ui::dialog::{
        assert_dialog_unconfirmed, cancel_test_dialog, submit_test_value, with_test_driver,
    };

    #[test]
    fn rejected_page_input_and_cancel_preserve_page_and_edited_pixels() {
        let dir = TempDir::new("invalid_page");
        write_png(&dir.join("a.png"), &solid_image(4, 3, [10, 20, 30, 255]));
        write_png(&dir.join("b.png"), &solid_image(4, 3, [40, 50, 60, 255]));
        let app = TestApp::new();
        app.with_app(|state| state.document.open(&dir.join("a.png")).unwrap());
        app.with_app(super::super::AppWindow::process_document_events);
        app.with_app(|state| state.execute_action(Action::InvertColors));
        let index = app.borrow().document.file_list().current_index();
        let pixels = app.borrow().document.current_image().unwrap().data.clone();
        assert!(app.borrow().document.has_unsaved_edit());
        with_test_driver(
            |hwnd| {
                for input in ["0", "3", "abc"] {
                    submit_test_value(hwnd, 0, input);
                    assert_dialog_unconfirmed(hwnd);
                }
                cancel_test_dialog(hwnd);
            },
            || app.with_app(|state| state.execute_action(Action::NavigateToPage)),
        );
        assert_eq!(app.borrow().document.file_list().current_index(), index);
        assert_eq!(app.borrow().document.current_image().unwrap().data, pixels);
        assert!(app.borrow().document.has_unsaved_edit());
        app.destroy();
    }

    #[test]
    fn slideshow_keeps_unedited_selection_and_discards_edited_selection() {
        let dir = TempDir::new("navigation_selection");
        for index in 0..2 {
            write_png(
                &dir.join(format!("image{index}.png")),
                &solid_image(20, 20, [10, 20, 30, 255]),
            );
        }
        let app = TestApp::new();
        app.with_app(|state| state.document.open_folder(&dir).unwrap());
        app.with_app(super::super::AppWindow::process_document_events);
        app.with_app(super::super::AppWindow::on_paint);
        app.drag_select((2, 2), (10, 10));
        let selected = app.borrow().selection.current_rect();
        assert!(selected.is_some());
        app.with_app(|state| state.slideshow_active = true);
        app.with_app(|state| state.slideshow_repeat = true);
        app.with_app(super::super::AppWindow::on_slideshow_timer);
        assert_eq!(app.borrow().selection.current_rect(), selected);
        assert_eq!(app.borrow().document.file_list().current_index(), Some(1));
        // 編集済み画像もスライドショーの自動送りで破棄する。
        app.with_app(|state| {
            state
                .document
                .apply_edit(solid_image(20, 20, [80, 90, 100, 255]));
        });
        app.with_app(super::super::AppWindow::on_slideshow_timer);
        assert!(!app.borrow().document.has_unsaved_edit());
        assert!(app.borrow().selection.current_rect().is_none());
        assert_eq!(app.borrow().document.file_list().current_index(), Some(0));
        app.destroy();
    }

    /// 有効なページ指定では、未編集の選択を保持し、編集済みの選択を解除する。
    #[test]
    fn page_dialog_keeps_unedited_selection_and_discards_edited_selection() {
        let dir = TempDir::new("page_navigation_selection");
        for index in 0..2 {
            write_png(
                &dir.join(format!("image{index}.png")),
                &solid_image(20, 20, [10, 20, 30, 255]),
            );
        }
        let app = TestApp::new();
        app.with_app(|state| state.document.open_folder(&dir).unwrap());
        app.with_app(super::super::AppWindow::process_document_events);
        app.with_app(super::super::AppWindow::on_paint);
        app.drag_select((2, 2), (10, 10));
        let selected = app.borrow().selection.current_rect();
        assert!(selected.is_some());
        crate::ui::dialog::with_test_driver(
            |hwnd| crate::ui::dialog::submit_test_value(hwnd, 0, "2"),
            || app.with_app(super::super::AppWindow::navigate_to_page_dialog),
        );
        assert_eq!(app.borrow().document.file_list().current_index(), Some(1));
        assert_eq!(app.borrow().selection.current_rect(), selected);
        app.with_app(|state| {
            state
                .document
                .apply_edit(solid_image(20, 20, [80, 90, 100, 255]));
        });
        crate::ui::dialog::with_test_driver(
            |hwnd| crate::ui::dialog::submit_test_value(hwnd, 0, "1"),
            || app.with_app(super::super::AppWindow::navigate_to_page_dialog),
        );
        assert_eq!(app.borrow().document.file_list().current_index(), Some(0));
        assert!(!app.borrow().document.has_unsaved_edit());
        assert!(app.borrow().selection.current_rect().is_none());
        app.destroy();
    }
}
