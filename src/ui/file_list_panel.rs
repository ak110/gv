use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::file_info::FileInfo;

pub const FILE_LIST_CONTROL_ID: u16 = 0x100;

const DEFAULT_WIDTH: i32 = 250;

const SUBCLASS_ID: usize = 1;

#[derive(Clone)]
pub struct FileListPanel {
    listview: HWND,
    requested_visible: bool,
    visible: bool,
    width: i32,
}

impl FileListPanel {
    pub fn create(parent: HWND) -> Self {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: u32::try_from(std::mem::size_of::<INITCOMMONCONTROLSEX>()).unwrap_or(0),
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        unsafe {
            let _ = InitCommonControlsEx(&raw const icc);
        }

        let listview = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::w!("SysListView32"),
                None,
                WINDOW_STYLE(
                    WS_CHILD.0
                        | WS_VSCROLL.0
                        | WS_BORDER.0
                        | LVS_REPORT
                        | LVS_OWNERDATA
                        | LVS_SINGLESEL
                        | LVS_NOCOLUMNHEADER
                        | LVS_SHOWSELALWAYS,
                ),
                0,
                0,
                DEFAULT_WIDTH,
                600,
                Some(parent),
                Some(HMENU(FILE_LIST_CONTROL_ID as *mut _)),
                None,
                None,
            )
            .unwrap_or_default()
        };

        let ex_style: u32 = LVS_EX_DOUBLEBUFFER | LVS_EX_FULLROWSELECT;
        unsafe {
            SendMessageW(
                listview,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                Some(WPARAM(ex_style as usize)),
                Some(LPARAM(ex_style as isize)),
            );
        }

        let mut col = LVCOLUMNW {
            mask: LVCOLUMNW_MASK(LVCF_WIDTH.0 | LVCF_FMT.0),
            fmt: LVCOLUMNW_FORMAT(LVCFMT_LEFT.0),
            cx: DEFAULT_WIDTH - 24,
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                listview,
                LVM_INSERTCOLUMNW,
                Some(WPARAM(0)),
                Some(LPARAM(std::ptr::from_mut(&mut col) as isize)),
            );
        }

        unsafe {
            let _ = SetWindowSubclass(
                listview,
                Some(listview_subclass_proc),
                SUBCLASS_ID,
                parent.0 as usize,
            );
        }

        Self {
            listview,
            requested_visible: false,
            visible: false,
            width: DEFAULT_WIDTH,
        }
    }

    pub fn toggle(&mut self, fullscreen: bool) {
        self.requested_visible = !self.requested_visible;
        self.sync_visibility(fullscreen);
    }

    pub fn sync_visibility(&mut self, fullscreen: bool) {
        self.visible = self.requested_visible && !fullscreen;
    }

    pub fn apply_visibility(&self) {
        let cmd = if self.visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            let _ = ShowWindow(self.listview, cmd);
        }
    }

    pub fn panel_width(&self) -> i32 {
        if self.visible { self.width } else { 0 }
    }

    pub fn resize(&self, parent_height: i32) {
        if self.visible {
            unsafe {
                let _ = MoveWindow(self.listview, 0, 0, self.width, parent_height, true);
            }
        }
    }

    pub fn update(&self, count: usize) {
        unsafe {
            // LVSICF_NOINVALIDATEALL: 可視範囲外は再描画しない
            // LVSICF_NOSCROLL: スクロール位置を維持
            // LVSICF_NOINVALIDATEALL=0x1, LVSICF_NOSCROLL=0x2
            const FLAGS: isize = 0x1 | 0x2;
            SendMessageW(
                self.listview,
                LVM_SETITEMCOUNT,
                Some(WPARAM(count)),
                Some(LPARAM(FLAGS)),
            );
            let _ = InvalidateRect(Some(self.listview), None, true);
        }
    }

    pub fn update_item(&self, index: usize) {
        let i = i32::try_from(index).unwrap_or(i32::MAX);
        unsafe {
            SendMessageW(
                self.listview,
                LVM_REDRAWITEMS,
                Some(WPARAM(i as usize)),
                Some(LPARAM(i as isize)),
            );
        }
    }

    pub fn format_label(info: &FileInfo, is_cached: bool) -> String {
        let mark = if info.marked { "\u{2605}" } else { "\u{3000}" };
        let cache = if is_cached { "\u{25CF}" } else { "\u{25CB}" };
        format!("{mark}{cache} {}", info.file_name)
    }

    pub fn set_selection(&self, index: usize) {
        let i = i32::try_from(index).unwrap_or(i32::MAX);
        let sel_focus = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
        unsafe {
            let current = SendMessageW(
                self.listview,
                LVM_GETNEXTITEM,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(LVNI_SELECTED as isize)),
            )
            .0;
            if current == i as isize {
                return;
            }

            let mut clear = LVITEMW {
                stateMask: sel_focus,
                state: LIST_VIEW_ITEM_STATE_FLAGS(0),
                ..Default::default()
            };
            SendMessageW(
                self.listview,
                LVM_SETITEMSTATE,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(std::ptr::from_mut(&mut clear) as isize)),
            );

            let mut item = LVITEMW {
                stateMask: sel_focus,
                state: sel_focus,
                ..Default::default()
            };
            SendMessageW(
                self.listview,
                LVM_SETITEMSTATE,
                Some(WPARAM(i as usize)),
                Some(LPARAM(std::ptr::from_mut(&mut item) as isize)),
            );

            let visible_count = SendMessageW(self.listview, LVM_GETCOUNTPERPAGE, None, None).0;
            let total = SendMessageW(self.listview, LVM_GETITEMCOUNT, None, None).0;
            if total <= 0 {
                return;
            }
            let margin = (visible_count * 2 / 5).max(0);
            let target = i as isize;
            let high = (target + margin).min(total - 1).max(0);
            let low = (target - margin).max(0);
            SendMessageW(
                self.listview,
                LVM_ENSUREVISIBLE,
                Some(WPARAM(high as usize)),
                Some(LPARAM(0)),
            );
            SendMessageW(
                self.listview,
                LVM_ENSUREVISIBLE,
                Some(WPARAM(low as usize)),
                Some(LPARAM(0)),
            );
        }
    }

    pub fn listview_hwnd(&self) -> HWND {
        self.listview
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn requested_visible(&self) -> bool {
        self.requested_visible
    }
}

unsafe extern "system" fn listview_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid: usize,
    parent_ptr: usize,
) -> LRESULT {
    let parent = HWND(parent_ptr as *mut _);

    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            unsafe {
                let _ = PostMessageW(Some(parent), msg, wparam, lparam);
            }
            return LRESULT(0);
        }
        WM_CHAR => {
            return LRESULT(0);
        }
        _ => {}
    }

    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
