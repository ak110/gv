//! ウィンドウ所有者と、アプリの借用を終了してから実行するWin32操作。

use std::cell::{Cell, Ref, RefCell};
use std::collections::VecDeque;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{AppWindow, WM_DOCUMENT_EVENT};

pub(super) enum UiEffect {
    Call(Box<dyn FnOnce()>),
    WithState(Box<dyn FnOnce(&WindowState)>),
    Modal(Box<dyn FnOnce(&WindowState)>),
}

/// HWNDへ公開する安定した所有者。再入時にも可変参照を直接生成しない。
pub(crate) struct WindowState {
    app: RefCell<AppWindow>,
    hwnd: HWND,
    destroyed: Cell<bool>,
    cleaned_up: Cell<bool>,
    flushing: Cell<bool>,
    modal: Cell<bool>,
    pending_paint: Cell<bool>,
    pending_size: Cell<Option<(u32, u32)>>,
    pending_document: Cell<bool>,
    pending_messages: RefCell<VecDeque<(u32, WPARAM, LPARAM)>>,
}

impl WindowState {
    pub(super) fn new(app: AppWindow) -> Self {
        Self {
            hwnd: app.hwnd,
            app: RefCell::new(app),
            destroyed: Cell::new(false),
            cleaned_up: Cell::new(false),
            flushing: Cell::new(false),
            modal: Cell::new(false),
            pending_paint: Cell::new(false),
            pending_size: Cell::new(None),
            pending_document: Cell::new(false),
            pending_messages: RefCell::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub(crate) fn borrow(&self) -> Ref<'_, AppWindow> {
        self.app.borrow()
    }

    pub(crate) fn with_app<R>(&self, f: impl FnOnce(&mut AppWindow) -> R) -> R {
        let result = f(&mut self.app.borrow_mut());
        self.flush();
        result
    }

    fn flush(&self) {
        if self.flushing.replace(true) {
            return;
        }
        loop {
            if self.destroyed.get() {
                let mut app = self.app.borrow_mut();
                if !self.cleaned_up.replace(true) {
                    app.pasted_images.remove_all();
                }
                app.effects.get_mut().clear();
                drop(app);
                for (msg, wparam, _) in self.pending_messages.borrow_mut().drain(..) {
                    if msg == WM_DROPFILES {
                        unsafe {
                            windows::Win32::UI::Shell::DragFinish(
                                windows::Win32::UI::Shell::HDROP(wparam.0 as *mut _),
                            );
                        }
                    }
                }
                break;
            }
            if let Some((width, height)) = self.pending_size.take() {
                self.app.borrow_mut().on_size(width, height);
            }
            if self.pending_paint.replace(false) {
                self.app.borrow().invalidate();
            }
            if !self.modal.get() && self.pending_document.replace(false) {
                self.app.borrow_mut().process_document_events();
            }
            let effect = {
                let app = self.app.borrow();
                let mut effects = app.effects.borrow_mut();
                let index = if self.modal.get() {
                    effects
                        .iter()
                        .position(|effect| !matches!(effect, UiEffect::Modal(_)))
                } else {
                    (!effects.is_empty()).then_some(0)
                };
                index.and_then(|index| effects.remove(index))
            };
            if let Some(effect) = effect {
                match effect {
                    UiEffect::Call(call) => call(),
                    UiEffect::WithState(call) => call(self),
                    UiEffect::Modal(call) => {
                        // モーダル待機中は通常の再入処理と、そのUI効果を完了できる。
                        self.flushing.set(false);
                        call(self);
                        self.flushing.set(true);
                    }
                }
                continue;
            }
            if self.modal.get() {
                break;
            }
            let message = self.pending_messages.borrow_mut().pop_front();
            if let Some((msg, wparam, lparam)) = message {
                self.dispatch(msg, wparam, lparam);
                continue;
            }
            break;
        }
        self.flushing.set(false);
    }

    pub(super) fn dispatch(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        if msg == WM_DESTROY {
            if !self.destroyed.replace(true) {
                crate::ui::window::set_window_data::<Self>(self.hwnd, std::ptr::null_mut());
                unsafe {
                    PostQuitMessage(0);
                }
            }
            if let Ok(mut app) = self.app.try_borrow_mut() {
                if !self.cleaned_up.replace(true) {
                    app.pasted_images.remove_all();
                }
                app.effects.get_mut().clear();
            }
            return Some(LRESULT(0));
        }
        if msg == WM_ERASEBKGND {
            return Some(LRESULT(1));
        }
        if self.destroyed.get() {
            return None;
        }
        if msg == WM_DOCUMENT_EVENT && self.modal.get() {
            self.pending_document.set(true);
            return Some(LRESULT(0));
        }
        if self.modal.get() && msg == WM_DROPFILES {
            // HDROPはDragFinishまで受信側が所有する。モーダル中に文書を変更しない。
            self.pending_messages
                .borrow_mut()
                .push_back((msg, wparam, lparam));
            return Some(LRESULT(0));
        }
        if self.modal.get() && msg == WM_NOTIFY {
            // SAFETY: 同期WM_NOTIFYのlparamは通知中有効なNMHDRを指す。コードだけをコピーする。
            let code = unsafe { (*(lparam.0 as *const windows::Win32::UI::Controls::NMHDR)).code };
            if code == windows::Win32::UI::Controls::LVN_ITEMCHANGED {
                return Some(LRESULT(0));
            }
        }
        // 親が無効化されるモーダル中に、残っていた入力やタイマーで別の操作を始めない。
        if self.modal.get()
            && matches!(
                msg,
                WM_TIMER
                    | WM_KEYDOWN
                    | WM_SYSKEYDOWN
                    | WM_COMMAND
                    | WM_MOUSEWHEEL
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_LBUTTONDBLCLK
                    | WM_MBUTTONUP
                    | WM_MOUSEMOVE
            )
        {
            return Some(LRESULT(0));
        }
        if !matches!(
            msg,
            WM_PAINT
                | WM_SIZE
                | WM_KEYDOWN
                | WM_SYSKEYDOWN
                | WM_MOUSEWHEEL
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_LBUTTONDBLCLK
                | WM_MBUTTONUP
                | WM_MOUSEMOVE
                | WM_SETCURSOR
                | WM_TIMER
                | WM_INITMENUPOPUP
                | WM_COMMAND
                | WM_NOTIFY
                | WM_DROPFILES
        ) && msg != WM_DOCUMENT_EVENT
        {
            return None;
        }
        let Ok(mut app) = self.app.try_borrow_mut() else {
            match msg {
                WM_PAINT => {
                    unsafe {
                        let _ = ValidateRect(Some(self.hwnd), None);
                    }
                    self.pending_paint.set(true);
                }
                WM_SIZE => self.pending_size.set(Some((
                    (lparam.0 & 0xFFFF) as u32,
                    ((lparam.0 >> 16) & 0xFFFF) as u32,
                ))),
                WM_DOCUMENT_EVENT => self.pending_document.set(true),
                // これらのメッセージはポインターを含まない値だけを保留する。
                WM_KEYDOWN | WM_SYSKEYDOWN | WM_MOUSEWHEEL | WM_LBUTTONDOWN | WM_LBUTTONUP
                | WM_LBUTTONDBLCLK | WM_MBUTTONUP | WM_MOUSEMOVE | WM_TIMER | WM_COMMAND
                | WM_DROPFILES => self
                    .pending_messages
                    .borrow_mut()
                    .push_back((msg, wparam, lparam)),
                _ => {}
            }
            return Some(LRESULT(0));
        };
        let result = app.handle_message(self.hwnd, msg, wparam, lparam);
        drop(app);
        self.flush();
        result
    }
}

impl AppWindow {
    /// 同期再入を起こし得る操作は、アプリへの借用を終了した後に実行する。
    pub(super) fn defer_ui(&self, call: impl FnOnce() + 'static) {
        self.effects
            .borrow_mut()
            .push_back(UiEffect::Call(Box::new(call)));
    }

    /// Win32操作の結果を、操作の終了後に短いアプリ借用で適用する。
    pub(super) fn defer_call<T: 'static>(
        &self,
        run: impl FnOnce() -> T + 'static,
        finish: impl FnOnce(&mut Self, T) + 'static,
    ) {
        self.effects
            .borrow_mut()
            .push_back(UiEffect::WithState(Box::new(move |state| {
                let result = run();
                if !state.destroyed.get() {
                    state.with_app(|app| finish(app, result));
                }
            })));
    }

    /// モーダルに必要な所有値と結果適用だけを保持し、待機中にアプリを借用しない。
    pub(super) fn defer_modal<T: 'static>(
        &mut self,
        run: impl FnOnce() -> T + 'static,
        finish: impl FnOnce(&mut Self, T) + 'static,
    ) {
        self.effects
            .borrow_mut()
            .push_back(UiEffect::Modal(Box::new(move |state| {
                state.modal.set(true);
                state.with_app(AppWindow::prepare_modal_dialog);
                let result = run();
                if !state.destroyed.get() {
                    state.with_app(|app| {
                        app.finish_modal_dialog();
                        finish(app, result);
                    });
                }
                state.modal.set(false);
            })));
    }
}
