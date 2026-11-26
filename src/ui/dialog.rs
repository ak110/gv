use std::rc::Rc;
use std::sync::OnceLock;

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH, UpdateWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::util::to_wide;

const CLASS_NAME: &str = "gv_form_dialog";
pub(crate) const ID_EDIT_BASE: i32 = 0x600;
const EM_SETSEL: u32 = 0x00B1;
static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();

struct DialogData {
    on_confirm: Rc<dyn Fn(HWND) -> bool>,
    closed: bool,
}

/// GWLP_USERDATAから型付きポインタを取得する。
///
/// # Safety
/// USERDATAがnullでなければ、生存するTを指す型付きポインタであること。
/// この関数は参照を生成しない。参照へ変換する側が、生存期間と共有・排他の条件を守ること。
pub unsafe fn get_window_data<T>(hwnd: HWND) -> Option<std::ptr::NonNull<T>> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut T;
    std::ptr::NonNull::new(ptr)
}

struct DialogWindow(HWND);

impl Drop for DialogWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.0, GWLP_USERDATA, 0);
            if IsWindow(Some(self.0)).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}

pub(super) fn show_dialog(
    parent: HWND,
    title: &str,
    (width, height): (i32, i32),
    create_content: impl FnOnce(HWND) -> Result<()>,
    on_confirm: impl Fn(HWND) -> bool + 'static,
) -> Result<()> {
    REGISTERED
        .get_or_init(|| register_class().map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut data = Box::new(DialogData {
        on_confirm: Rc::new(on_confirm),
        closed: false,
    });
    let (x, y) = super::center_on_parent(parent, width, height);
    let class = to_wide(CLASS_NAME);
    let title = to_wide(title);
    // SAFETY: dataのBoxはウィンドウ破棄まで生存する。WM_CREATEへ同じ型のポインタを渡し、
    // 処理中はdataへのRust参照を保持しない。文字列のバッファは同期呼出の終了まで生存する。
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            width,
            height,
            Some(parent),
            None,
            Some(GetModuleHandleW(None)?.into()),
            Some(std::ptr::from_mut(&mut *data).cast()),
        )
    }
    .context("ダイアログの作成に失敗しました")?;
    let window = DialogWindow(hwnd);
    create_content(hwnd)?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        if let Ok(edit) = GetDlgItem(Some(hwnd), ID_EDIT_BASE) {
            select_edit(edit);
        }
    }
    #[cfg(test)]
    TEST_DRIVER.with(|driver| {
        let callback = driver.borrow_mut().take();
        if let Some(callback) = callback {
            callback(hwnd);
        }
    });
    // SAFETY: Box内のclosedはループとウィンドウ破棄の完了まで生存する。
    // コールバックが変更する間はdataを借用しない。
    unsafe {
        run_modal_loop(parent, hwnd, &raw const data.closed);
    }
    drop(window);
    Ok(())
}

fn register_class() -> Result<()> {
    let class = to_wide(CLASS_NAME);
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(dialog_wnd_proc),
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as *mut _),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(std::ptr::from_ref(&wc)) == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
    }
    Ok(())
}

pub(super) fn create_control(
    hwnd: HWND,
    class: &str,
    text: &str,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
    id: i32,
    rect: (i32, i32, i32, i32),
) -> Result<HWND> {
    let class = to_wide(class);
    let text = to_wide(text);
    let (x, y, width, height) = rect;
    // SAFETY: NUL終端のクラス名とテキストは同期呼出が戻るまで生存する。
    Ok(unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class.as_ptr()),
            PCWSTR(text.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(hwnd),
            (id != 0).then_some(HMENU(id as usize as *mut _)),
            None,
            None,
        )
    }?)
}

pub(super) fn client_size(hwnd: HWND) -> Result<(i32, i32)> {
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(hwnd, std::ptr::from_mut(&mut rect))?;
    }
    Ok((rect.right - rect.left, rect.bottom - rect.top))
}

pub(super) fn select_edit(edit: HWND) {
    unsafe {
        let _ = SetFocus(Some(edit));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    }
}

unsafe extern "system" fn dialog_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        // SAFETY: WindowsがWM_CREATEへ渡すCREATESTRUCTWはこの呼出中に有効。
        // lpCreateParamsはshow_dialogでBoxのDialogDataへのポインタとして渡した。
        unsafe {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        }
        return LRESULT(0);
    }
    if msg == WM_CLOSE || (msg == WM_COMMAND && matches!(wparam.0 & 0xFFFF, 1 | 2)) {
        // SAFETY: このクラスのUSERDATAは生存するDialogData。参照から値を取り出した後、
        // SendMessage等によるメッセージ再入の前に参照の使用を終える。
        let Some(data) =
            (unsafe { get_window_data::<DialogData>(hwnd).map(|mut ptr| ptr.as_mut()) })
        else {
            return LRESULT(0);
        };
        if msg == WM_CLOSE || wparam.0 & 0xFFFF == 2 {
            data.closed = true;
            return LRESULT(0);
        }
        let on_confirm = Rc::clone(&data.on_confirm);
        if on_confirm(hwnd) {
            // SAFETY: 非nullのUSERDATAは生存するDialogDataを指し、再入の終了後にだけ排他的に参照する。
            if let Some(data) =
                unsafe { get_window_data::<DialogData>(hwnd).map(|mut ptr| ptr.as_mut()) }
            {
                data.closed = true;
            }
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// # Safety
/// hwndとparentは有効なウィンドウで、closedはループ終了まで生存する値を指すこと。
pub unsafe fn run_modal_loop(parent: HWND, hwnd: HWND, closed: *const bool) {
    // SAFETY: closedの生存期間は呼出側の契約。参照を保持せず、メッセージ処理の間に読み取る。
    unsafe {
        let was_disabled = EnableWindow(parent, false);
        let mut msg = MSG::default();
        loop {
            if *closed {
                break;
            }
            let ret = GetMessageW(std::ptr::from_mut(&mut msg), None, 0, 0);
            if ret.0 <= 0 {
                if ret.0 == 0 {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            if !IsDialogMessageW(hwnd, std::ptr::from_ref(&msg)).as_bool() {
                let _ = TranslateMessage(std::ptr::from_ref(&msg));
                DispatchMessageW(std::ptr::from_ref(&msg));
            }
        }
        if !was_disabled.as_bool() {
            let _ = EnableWindow(parent, true);
        }
        let _ = SetForegroundWindow(parent);
    }
}

#[cfg(test)]
type TestDriver = Box<dyn FnOnce(HWND)>;
#[cfg(test)]
thread_local! { static TEST_DRIVER: std::cell::RefCell<Option<TestDriver>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
pub(crate) fn with_test_driver<T>(
    driver: impl FnOnce(HWND) + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct ResetDriver;
    impl Drop for ResetDriver {
        fn drop(&mut self) {
            TEST_DRIVER.with(|driver| {
                driver.borrow_mut().take();
            });
        }
    }
    TEST_DRIVER.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(driver)).is_none());
    });
    let _reset = ResetDriver;
    action()
}

#[cfg(test)]
pub(crate) fn assert_dialog_unconfirmed(hwnd: HWND) {
    // SAFETY: テストドライバーはshow_dialogのBoxが生存する間に同期実行する。
    // メッセージ送信の完了後だけ状態を読み、参照を保持しない。
    let data = unsafe { get_window_data::<DialogData>(hwnd).map(|ptr| ptr.as_ref()) }
        .expect("dialog data");
    assert!(!data.closed);
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
}

#[cfg(test)]
pub(crate) fn submit_test_value(hwnd: HWND, index: i32, text: &str) {
    let text = to_wide(text);
    unsafe {
        let edit = GetDlgItem(Some(hwnd), ID_EDIT_BASE + index).expect("edit control");
        SetWindowTextW(edit, PCWSTR(text.as_ptr())).expect("set input");
        SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(1)), Some(LPARAM(0)));
    }
}

#[cfg(test)]
pub(crate) fn cancel_test_dialog(hwnd: HWND) {
    unsafe {
        SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(2)), Some(LPARAM(0)));
    }
}

#[cfg(test)]
pub(crate) fn assert_test_input_selected(hwnd: HWND, index: i32, expected_len: u32) {
    let (mut start, mut end) = (0u32, 0u32);
    // SAFETY: EM_GETSELへ渡すstartとendは同期呼出が完了するまで生存するu32。
    unsafe {
        let edit = GetDlgItem(Some(hwnd), ID_EDIT_BASE + index).expect("edit control");
        assert_eq!(
            windows::Win32::UI::Input::KeyboardAndMouse::GetFocus(),
            edit
        );
        SendMessageW(
            edit,
            0x00B0,
            Some(WPARAM(std::ptr::from_mut(&mut start) as usize)),
            Some(LPARAM(std::ptr::from_mut(&mut end) as isize)),
        );
    }
    assert_eq!((start, end), (0, expected_len));
}
