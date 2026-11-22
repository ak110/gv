pub type SusieResult = i32;

pub const SUSIE_SUCCESS: SusieResult = 0;

/// int GetPluginInfo(int infono, LPSTR buf, int buflen)
pub type GetPluginInfoFn = unsafe extern "system" fn(infono: i32, buf: *mut u8, buflen: i32) -> i32;

/// int IsSupported(LPCSTR filename, void* dw)
/// dw: ファイルハンドルまたはファイル先頭データへのポインタ
pub type IsSupportedFn = unsafe extern "system" fn(filename: *const u8, dw: *const u8) -> i32;

/// int GetPicture(LPCSTR buf, LONG_PTR len, unsigned int flag,
///                HANDLE* pHBInfo, HANDLE* pHBm, SUSIE_PROGRESS lpPrgressCallback, LONG_PTR lData)
/// flag: 0=ファイル名, 1=メモリイメージ
pub type GetPictureFn = unsafe extern "system" fn(
    buf: *const u8,
    len: isize,
    flag: u32,
    p_hb_info: *mut isize,
    p_hb_m: *mut isize,
    progress_callback: isize,
    l_data: isize,
) -> SusieResult;

/// int GetArchiveInfo(LPCSTR buf, LONG_PTR len, unsigned int flag, HLOCAL* lphInf)
pub type GetArchiveInfoFn = unsafe extern "system" fn(
    buf: *const u8,
    len: isize,
    flag: u32,
    lp_h_inf: *mut isize,
) -> SusieResult;

/// int GetFile(LPCSTR src, LONG_PTR len, LPSTR dest, unsigned int flag,
///             SUSIE_PROGRESS progressCallback, LONG_PTR lData)
/// flag: bit0 = 0:ファイル出力, 1:メモリ出力
///       bit8 = 0:srcはファイル名, 1:srcはメモリイメージ
pub type GetFileFn = unsafe extern "system" fn(
    src: *const u8,
    len: isize,
    dest: *mut u8,
    flag: u32,
    progress_callback: isize,
    l_data: isize,
) -> SusieResult;

#[repr(C, packed(1))]
#[derive(Clone)]
pub struct ArchiveFileInfo {
    pub method: [u8; 8],
    pub position: u64,
    pub compsize: u64,
    pub filesize: u64,
    pub timestamp: i64,
    pub path: [u8; 200],
    pub filename: [u8; 200],
    pub crc: u32,
}

impl ArchiveFileInfo {
    pub fn is_terminator(&self) -> bool {
        self.method.iter().all(|&b| b == 0) && self.filename.iter().all(|&b| b == 0)
    }
}

const _: () = assert!(std::mem::size_of::<ArchiveFileInfo>() == 444);
