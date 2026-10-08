//! 画像編集・フィルタダイアログ群
//!
//! AppWindowの画像編集関連アクションをまとめる。
//! 回転、リサイズ、レベル補正、ガンマ、明るさ/コントラスト、モザイク、
//! ガウスぼかし、アンシャープマスク、およびそれらの永続フィルタ版。

use crate::filter_spec::{FilterKind, FilterSpec};

use super::AppWindow;

impl AppWindow {
    pub(crate) fn action_filter_for_action(&mut self, action: crate::action::Action) {
        if let Some((spec, persistent)) = crate::filter_spec::for_action(action) {
            self.action_filter(spec, persistent);
        }
    }

    pub(crate) fn action_rotate_arbitrary(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::rotate_dialog::show_rotate_dialog(hwnd),
            |app, result| {
                if let Some(degrees) = app.take_success("ダイアログの表示", result)
                    && let Some(img) = app.document.current_image()
                {
                    let result = crate::filter::transform::rotate_arbitrary(img, degrees);
                    app.selection.deselect();
                    app.document.apply_edit(result);
                    app.process_document_events();
                }
            },
        );
    }

    pub(crate) fn action_resize(&mut self) {
        let Some((w, h)) = self
            .document
            .current_image()
            .map(|img| (img.width, img.height))
        else {
            return;
        };
        let hwnd = self.hwnd;
        self.defer_modal(
            move || crate::ui::resize_dialog::show_resize_dialog(hwnd, w, h),
            |app, result| {
                if let Some((nw, nh)) = app.take_success("ダイアログの表示", result)
                    && let Some(img) = app.document.current_image()
                {
                    match crate::filter::transform::resize(img, nw, nh) {
                        Ok(result) => {
                            app.selection.deselect();
                            app.document.apply_edit(result);
                            app.process_document_events();
                        }
                        Err(e) => {
                            app.show_error_title(&format!("リサイズに失敗しました: {e:#}"));
                        }
                    }
                }
            },
        );
    }

    pub(crate) fn action_filter(&mut self, spec: &FilterSpec, persistent: bool) {
        if persistent {
            if self.remove_persistent_filter_if_exists(spec.kind) {
                return;
            }
        } else if self.document.current_image().is_none() {
            return;
        }
        if spec.parameters.is_empty() {
            self.apply_filter_values(spec, persistent, Vec::new());
        } else {
            let fields = spec.fields();
            let title = if persistent {
                format!("永続{}", spec.title)
            } else {
                spec.title.into()
            };
            let hwnd = self.hwnd;
            let spec = FilterSpec {
                kind: spec.kind,
                title: spec.title,
                action: spec.action,
                persistent_action: spec.persistent_action,
                parameters: spec.parameters,
            };
            self.defer_modal(
                move || crate::ui::filter_dialog::show_filter_dialog(hwnd, &title, &fields),
                move |app, result| {
                    if let Some(values) = app.take_success("ダイアログの表示", result) {
                        app.apply_filter_values(&spec, persistent, values);
                    }
                },
            );
        }
    }

    fn apply_filter_values(&mut self, spec: &FilterSpec, persistent: bool, values: Vec<String>) {
        let Some(operation) =
            self.take_success("フィルター入力の検証", spec.operation(&values).map(Some))
        else {
            return;
        };
        if persistent {
            self.document
                .persistent_filter_mut()
                .add_operation(operation);
            self.document.on_persistent_filter_changed();
        } else if let Some(image) = self.document.current_image() {
            let selection = self.selection.current_rect();
            let result =
                crate::persistent_filter::apply_operation(image, &operation, selection.as_ref());
            if matches!(
                spec.kind,
                FilterKind::FlipHorizontal
                    | FilterKind::FlipVertical
                    | FilterKind::Rotate180
                    | FilterKind::Rotate90CW
                    | FilterKind::Rotate90CCW
            ) {
                self.selection.deselect();
            }
            self.document.apply_edit(result);
        }
        self.process_document_events();
    }
}

impl AppWindow {
    /// パラメータ付き永続フィルタのトグル (既存なら削除してtrue、なければfalse)
    pub(super) fn remove_persistent_filter_if_exists(
        &mut self,
        kind: crate::filter_spec::FilterKind,
    ) -> bool {
        let pf = self.document.persistent_filter_mut();
        if pf.remove_operation_type(kind) {
            self.document.on_persistent_filter_changed();
            self.process_document_events();
            true
        } else {
            false
        }
    }

    pub(super) fn action_p_filter_toggle(&mut self) {
        self.document.persistent_filter_mut().toggle_enabled();
        self.document.on_persistent_filter_changed();
        self.process_document_events();
    }

    pub(super) fn action_crop(&mut self) {
        if let Some(sel_rect) = self.selection.current_rect()
            && let Some(img) = self.document.current_image()
        {
            let cropped = crate::filter::transform::crop(img, &sel_rect);
            self.selection.deselect();
            self.document.apply_edit(cropped);
            self.process_document_events();
            self.update_title();
        }
    }
}
