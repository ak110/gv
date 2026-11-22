use std::collections::HashSet;

use windows::Win32::Foundation::{LPARAM, LRESULT};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::UI::Controls::{
    LVIF_STATE, LVIF_TEXT, LVIS_SELECTED, LVN_GETDISPINFOW, LVN_ITEMCHANGED, NMLISTVIEW,
    NMLVDISPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::HMENU;

use crate::action::Action;
use crate::ui::file_list_panel::FileListPanel;
use crate::ui::menu;

use super::AppWindow;

impl AppWindow {
    pub(crate) fn process_document_events(&mut self) {
        self.document.process_prefetch_responses();

        self.document.process_expand_results();

        let mut file_list_changed = false;
        let mut nav_changed_index: Option<usize> = None;
        while let Ok(event) = self.event_receiver.try_recv() {
            match event {
                crate::document::DocumentEvent::ImageReady => unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                },
                crate::document::DocumentEvent::NavigationChanged { index, .. } => {
                    nav_changed_index = Some(index);
                }
                crate::document::DocumentEvent::FileListChanged => {
                    self.stop_slideshow();
                    file_list_changed = true;
                }
                crate::document::DocumentEvent::Error(notice) => {
                    self.show_error_notice(&notice);
                }
            }
        }

        if file_list_changed {
            let count = self.document.file_list().len();
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.update(count));
            self.update_title();
        }
        if let Some(index) = nav_changed_index {
            self.update_title();
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.set_selection(index));
        }

        if self.file_list_panel.is_visible() {
            let doc = &self.document;
            let len = doc.file_list().len();
            let mut new_cached = HashSet::new();
            for i in 0..len {
                if doc.is_cached(i) {
                    new_cached.insert(i);
                }
            }
            for &i in self.cached_indices.symmetric_difference(&new_cached) {
                let panel = self.file_list_panel.clone();
                self.defer_ui(move || panel.update_item(i));
            }
            self.cached_indices = new_cached;
        }
    }

    pub(crate) fn update_menu_checks(&self, popup: HMENU) {
        let pf = self.document.persistent_filter();
        let pf_enabled = pf.is_enabled();

        menu::update_menu_check(popup, Action::PFilterToggle, pf_enabled);

        for spec in crate::filter_spec::FILTER_SPECS {
            if let Some(action) = spec.persistent_action {
                menu::update_menu_check(popup, action, pf.has_operation(spec.kind));
                menu::update_menu_enabled(popup, action, pf_enabled);
            }
        }

        menu::update_menu_check(
            popup,
            Action::ToggleFileList,
            self.file_list_panel.requested_visible(),
        );
        menu::update_menu_check(popup, Action::ToggleAlwaysOnTop, self.always_on_top);
        menu::update_menu_check(
            popup,
            Action::ToggleMargin,
            self.renderer.layout().margin_enabled,
        );
        menu::update_menu_check(
            popup,
            Action::ToggleCursorHide,
            self.cursor_hider.is_enabled(),
        );
        menu::update_menu_check(
            popup,
            Action::ToggleFullscreen,
            self.fullscreen.is_fullscreen(),
        );
        menu::update_menu_check(popup, Action::SlideshowToggle, self.slideshow_active);
    }

    pub(crate) fn sync_file_list_panel(&mut self) {
        if self.file_list_panel.is_visible() {
            let doc = &self.document;
            let len = doc.file_list().len();
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.update(len));
            if let Some(idx) = doc.file_list().current_index() {
                let panel = self.file_list_panel.clone();
                self.defer_ui(move || panel.set_selection(idx));
            }
        }
    }

    pub(crate) fn handle_file_list_notify(&mut self, code: u32, lparam: LPARAM) -> LRESULT {
        match code {
            i if i == LVN_GETDISPINFOW => {
                // SAFETY: LVN_GETDISPINFOW の lparam は OS が有効な NMLVDISPINFOW へのポインタを保証する
                let dispinfo = unsafe { &mut *(lparam.0 as *mut NMLVDISPINFOW) };
                if (dispinfo.item.mask & LVIF_TEXT).0 != 0 {
                    let idx = dispinfo.item.iItem as usize;
                    let files = self.document.file_list().files();
                    if let Some(info) = files.get(idx) {
                        let is_cached = self.document.is_cached(idx);
                        let label = FileListPanel::format_label(info, is_cached);
                        let max = dispinfo.item.cchTextMax as usize;
                        if max > 0 && !dispinfo.item.pszText.0.is_null() {
                            let mut wide: Vec<u16> = label.encode_utf16().collect();
                            if wide.len() >= max {
                                wide.truncate(max - 1);
                            }
                            wide.push(0);
                            // SAFETY: pszText は OS が確保した cchTextMax 文字分のバッファを指す。
                            // wide.len() <= max (cchTextMax) を上記で保証済みのため範囲内に収まる。
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    wide.as_ptr(),
                                    dispinfo.item.pszText.0,
                                    wide.len(),
                                );
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            i if i == LVN_ITEMCHANGED => {
                // SAFETY: LVN_ITEMCHANGED の lparam は OS が有効な NMLISTVIEW へのポインタを保証する
                let nmlv = unsafe { &*(lparam.0 as *const NMLISTVIEW) };
                let became_selected = (nmlv.uChanged.0 & LVIF_STATE.0) != 0
                    && (nmlv.uOldState & LVIS_SELECTED.0) == 0
                    && (nmlv.uNewState & LVIS_SELECTED.0) != 0;
                if became_selected && nmlv.iItem >= 0 {
                    let target = nmlv.iItem as usize;
                    if self.document.file_list().current_index() != Some(target) {
                        self.begin_user_operation();
                        self.stop_slideshow();
                        self.navigate_with_guard(|document| document.navigate_to(target));
                    }
                }
                LRESULT(0)
            }
            _ => LRESULT(0),
        }
    }
}

impl AppWindow {
    pub(super) fn action_toggle_file_list(&mut self) {
        self.file_list_panel.toggle(self.fullscreen.is_fullscreen());
        let panel = self.file_list_panel.clone();
        self.defer_ui(move || panel.apply_visibility());
        if self.file_list_panel.is_visible() {
            let doc = &self.document;
            let len = doc.file_list().len();
            let panel = self.file_list_panel.clone();
            self.defer_ui(move || panel.update(len));
            if let Some(idx) = doc.file_list().current_index() {
                let panel = self.file_list_panel.clone();
                self.defer_ui(move || panel.set_selection(idx));
            }
            self.cached_indices.clear();
            for i in 0..len {
                if self.document.is_cached(i) {
                    self.cached_indices.insert(i);
                }
            }
        }
        let (width, height) = crate::ui::window::get_client_size(self.hwnd);
        self.on_size(width, height);
        let hwnd = self.hwnd;
        self.defer_ui(move || unsafe {
            let _ = UpdateWindow(hwnd);
        });
    }
}
