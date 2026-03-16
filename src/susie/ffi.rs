//! Susieプラグイン FFI型定義
//! Susie Plug-in API Specification (64bit)

/// GetPluginInfo / IsSupported / GetPicture等の戻り値
pub type SusieResult = i32;

/// 成功
pub const SUSIE_SUCCESS: SusieResult = 0;

/// GetPluginInfoの関数ポインタ型
/// int GetPluginInfo(int infono, LPSTR buf, int buflen)
pub type GetPluginInfoFn = unsafe extern "system" fn(infono: i32, buf: *mut u8, buflen: i32) -> i32;

/// IsSupportedの関数ポインタ型
/// int IsSupported(LPCSTR filename, void* dw)
/// dw: ファイルハンドルまたはファイル先頭データへのポインタ
pub type IsSupportedFn = unsafe extern "system" fn(filename: *const u8, dw: *const u8) -> i32;

/// GetPictureの関数ポインタ型
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

/// GetArchiveInfoの関数ポインタ型
/// int GetArchiveInfo(LPCSTR buf, LONG_PTR len, unsigned int flag, HLOCAL* lphInf)
pub type GetArchiveInfoFn = unsafe extern "system" fn(
    buf: *const u8,
    len: isize,
    flag: u32,
    lp_h_inf: *mut isize,
) -> SusieResult;

/// GetFileの関数ポインタ型
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

/// アーカイブファイル情報構造体（Susie API定義）
/// packed(1)で200+200+...のフィールドが隙間なく並ぶ
#[repr(C, packed(1))]
#[derive(Clone)]
pub struct ArchiveFileInfo {
    /// method: 圧縮法の種類
    pub method: [u8; 8],
    /// position: ファイル内での位置（GetFileで使用）
    pub position: u64,
    /// compsize: 圧縮サイズ
    pub compsize: u64,
    /// filesize: 元のサイズ
    pub filesize: u64,
    /// timestamp: ファイル更新日時
    pub timestamp: i64,
    /// path: 相対パス
    pub path: [u8; 200],
    /// filename: ファイル名
    pub filename: [u8; 200],
    /// crc: CRC
    pub crc: u32,
}

impl ArchiveFileInfo {
    /// 終端エントリか判定（methodが全て0）
    pub fn is_terminator(&self) -> bool {
        self.method.iter().all(|&b| b == 0) && self.filename.iter().all(|&b| b == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_file_info_size() {
        // ArchiveFileInfoは固定サイズであるべき
        // method(8) + position(8) + compsize(8) + filesize(8) + timestamp(8)
        // + path(200) + filename(200) + crc(4) = 444
        assert_eq!(std::mem::size_of::<ArchiveFileInfo>(), 444);
    }
}
