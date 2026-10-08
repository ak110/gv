//! 数値フォームと読み取り専用情報表示のウィンドウ基盤

use std::sync::OnceLock;

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH, HFONT, UpdateWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::form_dialog::FieldDef;
use crate::util::to_wide;

const CLASS_NAME: &str = "gv_form_dialog";
pub(crate) const ID_EDIT_BASE: i32 = 0x600;
const ID_ERROR: i32 = 0x700;
const EM_SETSEL: u32 = 0x00B1;
const MARGIN: i32 = 12;
const FIELD_HEIGHT: i32 = 48;
static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();

pub(super) enum DialogContent {
    Form(Vec<FieldDef>),
    Info { text: String, font: HFONT },
}

struct DialogData {
    fields: Vec<FieldDef>,
    results: Option<Vec<String>>,
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
    content: DialogContent,
) -> Result<Option<Vec<String>>> {
    REGISTERED
        .get_or_init(|| register_class().map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let fields = match &content {
        DialogContent::Form(fields) => fields.clone(),
        DialogContent::Info { .. } => Vec::new(),
    };
    let (width, height) = match &content {
        DialogContent::Form(fields) => (
            360,
            MARGIN * 4
                + fields.len() as i32 * FIELD_HEIGHT
                + 64
                + unsafe { GetSystemMetrics(SM_CYCAPTION) + GetSystemMetrics(SM_CYSIZEFRAME) },
        ),
        DialogContent::Info { .. } => (640, 480),
    };
    let mut data = Box::new(DialogData {
        fields,
        results: None,
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
    create_content(hwnd, &content)?;
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
    Ok(data.results.take())
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

fn create_control(
    hwnd: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: i32,
    rect: (i32, i32, i32, i32),
) -> Result<HWND> {
    let text = to_wide(text);
    let (x, y, width, height) = rect;
    Ok(unsafe {
        CreateWindowExW(
            if style.contains(WS_TABSTOP) && (ID_EDIT_BASE..ID_ERROR).contains(&id) {
                WS_EX_CLIENTEDGE
            } else {
                WINDOW_EX_STYLE::default()
            },
            class,
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

fn create_content(hwnd: HWND, content: &DialogContent) -> Result<()> {
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(hwnd, std::ptr::from_mut(&mut rect))?;
    }
    let width = rect.right - rect.left;
    match content {
        DialogContent::Form(fields) => {
            for (i, field) in fields.iter().enumerate() {
                let y = MARGIN + i as i32 * FIELD_HEIGHT;
                create_control(
                    hwnd,
                    w!("STATIC"),
                    &field.display_label(),
                    WS_CHILD | WS_VISIBLE,
                    0,
                    (MARGIN, y, width - MARGIN * 2, 20),
                )?;
                create_control(
                    hwnd,
                    w!("EDIT"),
                    &field.default,
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                    ID_EDIT_BASE + i as i32,
                    (MARGIN, y + 20, width - MARGIN * 2, 24),
                )?;
            }
            let y = MARGIN + fields.len() as i32 * FIELD_HEIGHT;
            create_control(
                hwnd,
                w!("STATIC"),
                "",
                WS_CHILD | WS_VISIBLE,
                ID_ERROR,
                (MARGIN, y, width - MARGIN * 2, 40),
            )?;
            let x = (width - 168) / 2;
            create_control(
                hwnd,
                w!("BUTTON"),
                "OK",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
                1,
                (x, y + 44, 80, 28),
            )?;
            create_control(
                hwnd,
                w!("BUTTON"),
                "キャンセル",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                2,
                (x + 88, y + 44, 80, 28),
            )?;
        }
        DialogContent::Info { text, font } => {
            let edit = create_control(
                hwnd,
                w!("EDIT"),
                &text.replace('\n', "\r\n"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
                ID_EDIT_BASE,
                (0, 0, width, rect.bottom - rect.top),
            )?;
            if !font.is_invalid() {
                unsafe {
                    SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }
        }
    }
    Ok(())
}

unsafe fn select_edit(edit: HWND) {
    unsafe {
        let _ = SetFocus(Some(edit));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    }
}

fn read_edit_text(edit: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        let mut text = vec![0u16; len + 1];
        let copied = GetWindowTextW(edit, &mut text).max(0) as usize;
        String::from_utf16_lossy(&text[..copied]).trim().to_owned()
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
        if msg == WM_CLOSE || wparam.0 & 0xFFFF == 2 || data.fields.is_empty() {
            data.closed = true;
            return LRESULT(0);
        }
        let fields = data.fields.clone();
        let values: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let edit =
                    unsafe { GetDlgItem(Some(hwnd), ID_EDIT_BASE + i as i32) }.unwrap_or_default();
                read_edit_text(edit)
            })
            .collect();
        if let Some((index, field)) = fields
            .iter()
            .enumerate()
            .find(|(i, field)| !field.rule.accepts(&values[*i]))
        {
            let message = to_wide(&format!(
                "{}に従って入力し直してください。",
                field.display_label()
            ));
            unsafe {
                if let Ok(error) = GetDlgItem(Some(hwnd), ID_ERROR) {
                    let _ = SetWindowTextW(error, PCWSTR(message.as_ptr()));
                }
                if let Ok(edit) = GetDlgItem(Some(hwnd), ID_EDIT_BASE + index as i32) {
                    select_edit(edit);
                }
            }
        } else {
            // SAFETY: 上と同じ型・生存期間。Win32呼出の完了後に改めて短期間だけ取得する。
            if let Some(data) =
                unsafe { get_window_data::<DialogData>(hwnd).map(|mut ptr| ptr.as_mut()) }
            {
                data.results = Some(values);
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
    assert!(data.results.is_none());
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
