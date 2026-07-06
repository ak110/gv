//! 画像編集・フィルタダイアログ群
//!
//! AppWindowの画像編集関連アクションをまとめる。
//! 回転、リサイズ、レベル補正、ガンマ、明るさ/コントラスト、モザイク、
//! ガウスぼかし、アンシャープマスク、およびそれらの永続フィルタ版。

use crate::persistent_filter::FilterOperation;

use super::AppWindow;

impl AppWindow {
    pub(crate) fn action_rotate_arbitrary(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        self.prepare_modal_dialog();
        let degrees_opt = crate::ui::rotate_dialog::show_rotate_dialog(self.hwnd);
        self.finish_modal_dialog();
        if let Some(degrees) = degrees_opt
            && let Some(img) = self.document.current_image()
        {
            let result = crate::filter::transform::rotate_arbitrary(img, degrees);
            self.selection.deselect();
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_resize(&mut self) {
        let Some((w, h)) = self
            .document
            .current_image()
            .map(|img| (img.width, img.height))
        else {
            return;
        };
        self.prepare_modal_dialog();
        let dialog_result = crate::ui::resize_dialog::show_resize_dialog(self.hwnd, w, h);
        self.finish_modal_dialog();
        if let Some((nw, nh)) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            match crate::filter::transform::resize(img, nw, nh) {
                Ok(result) => {
                    self.selection.deselect();
                    self.document.apply_edit(result);
                    self.process_document_events();
                }
                Err(e) => {
                    self.show_error_title(&format!("リサイズに失敗しました: {e}"));
                }
            }
        }
    }

    pub(crate) fn action_fill(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [
            FieldDef {
                label: "赤 (0-255)",
                default: "255".into(),
                integer_only: true,
            },
            FieldDef {
                label: "緑 (0-255)",
                default: "255".into(),
                integer_only: true,
            },
            FieldDef {
                label: "青 (0-255)",
                default: "255".into(),
                integer_only: true,
            },
        ];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "塗り潰す", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let r = vals[0].parse::<u8>().unwrap_or(255);
            let g = vals[1].parse::<u8>().unwrap_or(255);
            let b = vals[2].parse::<u8>().unwrap_or(255);
            let sel = self.selection.current_rect();
            let result = crate::filter::color::fill(img, sel.as_ref(), r, g, b);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_levels(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [
            FieldDef {
                label: "下限 (0-255)",
                default: "0".into(),
                integer_only: true,
            },
            FieldDef {
                label: "上限 (0-255)",
                default: "255".into(),
                integer_only: true,
            },
        ];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "レベル補正", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let low = vals[0].parse::<u8>().unwrap_or(0);
            let high = vals[1].parse::<u8>().unwrap_or(255);
            let sel = self.selection.current_rect();
            let result = crate::filter::brightness::levels(img, sel.as_ref(), low, high);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_gamma(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "ガンマ値 (0.1〜10.0)",
            default: "1.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "ガンマ補正", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let gamma = vals[0].parse::<f64>().unwrap_or(1.0).clamp(0.1, 10.0);
            let sel = self.selection.current_rect();
            let result = crate::filter::brightness::gamma(img, sel.as_ref(), gamma);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_brightness_contrast(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [
            FieldDef {
                label: "明るさ (-128〜128)",
                default: "0".into(),
                integer_only: false,
            },
            FieldDef {
                label: "コントラスト (-128〜128)",
                default: "0".into(),
                integer_only: false,
            },
        ];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "明るさとコントラスト", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let brightness = vals[0].parse::<i32>().unwrap_or(0).clamp(-128, 128);
            let contrast = vals[1].parse::<i32>().unwrap_or(0).clamp(-128, 128);
            let sel = self.selection.current_rect();
            let result = crate::filter::brightness::brightness_contrast(
                img,
                sel.as_ref(),
                brightness,
                contrast,
            );
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_mosaic(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "ブロックサイズ",
            default: "10".into(),
            integer_only: true,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "モザイク", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let size = vals[0].parse::<u32>().unwrap_or(10).max(1);
            let sel = self.selection.current_rect();
            let result = crate::filter::blur::mosaic(img, sel.as_ref(), size);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_gaussian_blur(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "半径 (0.1〜10.0)",
            default: "2.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "ガウスぼかし", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let radius = vals[0].parse::<f64>().unwrap_or(2.0).clamp(0.1, 10.0);
            let sel = self.selection.current_rect();
            let result = crate::filter::blur::gaussian_blur(img, sel.as_ref(), radius);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    pub(crate) fn action_unsharp_mask(&mut self) {
        if self.document.current_image().is_none() {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "半径 (0.1〜10.0)",
            default: "2.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "アンシャープマスク", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result
            && let Some(img) = self.document.current_image()
        {
            let radius = vals[0].parse::<f64>().unwrap_or(2.0).clamp(0.1, 10.0);
            let sel = self.selection.current_rect();
            let result = crate::filter::blur::unsharp_mask(img, sel.as_ref(), radius);
            self.document.apply_edit(result);
            self.process_document_events();
        }
    }

    // --- 永続フィルタ（パラメータあり） ---

    pub(crate) fn action_pfilter_levels(&mut self) {
        let probe = FilterOperation::Levels { low: 0, high: 0 };
        if self.remove_persistent_filter_if_exists(&probe) {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [
            FieldDef {
                label: "下限 (0-255)",
                default: "0".into(),
                integer_only: true,
            },
            FieldDef {
                label: "上限 (0-255)",
                default: "255".into(),
                integer_only: true,
            },
        ];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "永続レベル補正", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result {
            let low = vals[0].parse::<u8>().unwrap_or(0);
            let high = vals[1].parse::<u8>().unwrap_or(255);
            self.document
                .persistent_filter_mut()
                .add_operation(FilterOperation::Levels { low, high });
            self.document.on_persistent_filter_changed();
            self.process_document_events();
        }
    }

    pub(crate) fn action_pfilter_gamma(&mut self) {
        let probe = FilterOperation::Gamma { value: 0.0 };
        if self.remove_persistent_filter_if_exists(&probe) {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "ガンマ値 (0.1〜10.0)",
            default: "1.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "永続ガンマ補正", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result {
            let value = vals[0].parse::<f64>().unwrap_or(1.0).clamp(0.1, 10.0);
            self.document
                .persistent_filter_mut()
                .add_operation(FilterOperation::Gamma { value });
            self.document.on_persistent_filter_changed();
            self.process_document_events();
        }
    }

    pub(crate) fn action_pfilter_brightness_contrast(&mut self) {
        let probe = FilterOperation::BrightnessContrast {
            brightness: 0,
            contrast: 0,
        };
        if self.remove_persistent_filter_if_exists(&probe) {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [
            FieldDef {
                label: "明るさ (-128〜128)",
                default: "0".into(),
                integer_only: false,
            },
            FieldDef {
                label: "コントラスト (-128〜128)",
                default: "0".into(),
                integer_only: false,
            },
        ];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "永続明るさとコントラスト", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result {
            let brightness = vals[0].parse::<i32>().unwrap_or(0).clamp(-128, 128);
            let contrast = vals[1].parse::<i32>().unwrap_or(0).clamp(-128, 128);
            self.document.persistent_filter_mut().add_operation(
                FilterOperation::BrightnessContrast {
                    brightness,
                    contrast,
                },
            );
            self.document.on_persistent_filter_changed();
            self.process_document_events();
        }
    }

    pub(crate) fn action_pfilter_gaussian_blur(&mut self) {
        let probe = FilterOperation::GaussianBlur { radius: 0.0 };
        if self.remove_persistent_filter_if_exists(&probe) {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "半径 (0.1〜10.0)",
            default: "2.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "永続ガウスぼかし", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result {
            let radius = vals[0].parse::<f64>().unwrap_or(2.0).clamp(0.1, 10.0);
            self.document
                .persistent_filter_mut()
                .add_operation(FilterOperation::GaussianBlur { radius });
            self.document.on_persistent_filter_changed();
            self.process_document_events();
        }
    }

    pub(crate) fn action_pfilter_unsharp_mask(&mut self) {
        let probe = FilterOperation::UnsharpMask { radius: 0.0 };
        if self.remove_persistent_filter_if_exists(&probe) {
            return;
        }
        use crate::ui::filter_dialog::{FieldDef, show_filter_dialog};
        let fields = [FieldDef {
            label: "半径 (0.1〜10.0)",
            default: "2.0".into(),
            integer_only: false,
        }];
        self.prepare_modal_dialog();
        let dialog_result = show_filter_dialog(self.hwnd, "永続アンシャープマスク", &fields);
        self.finish_modal_dialog();
        if let Some(vals) = dialog_result {
            let radius = vals[0].parse::<f64>().unwrap_or(2.0).clamp(0.1, 10.0);
            self.document
                .persistent_filter_mut()
                .add_operation(FilterOperation::UnsharpMask { radius });
            self.document.on_persistent_filter_changed();
            self.process_document_events();
        }
    }
}
