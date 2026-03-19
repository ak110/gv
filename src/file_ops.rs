//! Win32 Shell APIによるファイル操作 + ダイアログ

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{FO_COPY, FO_DELETE, FO_MOVE, FOF_ALLOWUNDO, FOF_MULTIDESTFILES};
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, IFileOpenDialog, IFileSaveDialog, SHFILEOPSTRUCTW, SHFileOperationW,
};

use crate::util::to_wide;

/// SHFileOperationWの共通ラッパー
/// 成功時はtrue、ユーザーキャンセル時はfalseを返す
fn sh_file_operation(
    hwnd: HWND,
    func: u32,
    from: &[u16],
    to: Option<&[u16]>,
    flags: u16,
    error_msg: &str,
) -> Result<bool> {
    let mut op = SHFILEOPSTRUCTW {
        hwnd,
        wFunc: func,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        pTo: to.map_or(windows::core::PCWSTR::null(), |t| {
            windows::core::PCWSTR(t.as_ptr())
        }),
        fFlags: flags,
        ..Default::default()
    };

    let result = unsafe { SHFileOperationW(std::ptr::from_mut(&mut op)) };
    if result != 0 {
        if op.fAnyOperationsAborted.as_bool() {
            return Ok(false);
        }
        anyhow::bail!("{error_msg} (code: 0x{result:04X})");
    }
    Ok(!op.fAnyOperationsAborted.as_bool())
}

/// ごみ箱経由でファイルを削除する
pub fn delete_to_recycle_bin(hwnd: HWND, paths: &[&Path]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let from = build_multi_path_string(paths);
    sh_file_operation(
        hwnd,
        FO_DELETE,
        &from,
        None,
        FOF_ALLOWUNDO.0 as u16,
        "ファイル削除に失敗しました",
    )
}

/// ファイルを移動する（SHFileOperationW）
/// pToはディレクトリ（複数ファイルを一つのフォルダに移動）
pub fn move_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let from = build_multi_path_string(paths);
    let to = build_single_path_string(dest);
    sh_file_operation(
        hwnd,
        FO_MOVE,
        &from,
        Some(&to),
        FOF_ALLOWUNDO.0 as u16,
        &format!("ファイル移動に失敗しました\n  dest: {}", dest.display()),
    )
}

/// ファイルをコピーする（SHFileOperationW）
pub fn copy_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let from = build_multi_path_string(paths);
    let to = build_single_path_string(dest);
    sh_file_operation(
        hwnd,
        FO_COPY,
        &from,
        Some(&to),
        FOF_ALLOWUNDO.0 as u16,
        "ファイルコピーに失敗しました",
    )
}

/// ファイル選択ダイアログ（IFileOpenDialog）
pub fn open_file_dialog(hwnd: HWND, initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Shell::FileOpenDialog,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )
        .context("FileOpenDialog作成失敗")?;

        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST)?;

        // 初期ディレクトリ設定
        if let Some(dir) = initial_dir
            && let Some(item) = create_shell_item(dir)
        {
            dialog.SetFolder(&item)?;
        }

        // 画像ファイルフィルタ
        let filter_name: Vec<u16> = "画像ファイル\0".encode_utf16().collect();
        let filter_spec: Vec<u16> = "*.jpg;*.jpeg;*.png;*.gif;*.bmp;*.webp;*.tga;*.tiff;*.ico\0"
            .encode_utf16()
            .collect();
        let all_name: Vec<u16> = "すべてのファイル\0".encode_utf16().collect();
        let all_spec: Vec<u16> = "*.*\0".encode_utf16().collect();

        let filters = [
            windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR(filter_name.as_ptr()),
                pszSpec: windows::core::PCWSTR(filter_spec.as_ptr()),
            },
            windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR(all_name.as_ptr()),
                pszSpec: windows::core::PCWSTR(all_spec.as_ptr()),
            },
        ];
        dialog.SetFileTypes(&filters)?;

        match dialog.Show(Some(hwnd)) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == 0x800704C7 => return Ok(None), // ユーザーキャンセル
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// フォルダ選択ダイアログ（IFileOpenDialog + FOS_PICKFOLDERS）
pub fn open_folder_dialog(hwnd: HWND, initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    select_folder_dialog(hwnd, "フォルダを開く", initial_dir)
}

/// フォルダ選択ダイアログ（移動/コピー先選択用）
pub fn select_folder_dialog(
    hwnd: HWND,
    title: &str,
    initial_dir: Option<&Path>,
) -> Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Shell::FileOpenDialog,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )
        .context("FileOpenDialog作成失敗")?;

        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_PICKFOLDERS)?;

        let title_wide = to_wide(title);
        dialog.SetTitle(windows::core::PCWSTR(title_wide.as_ptr()))?;

        // 初期ディレクトリ設定
        if let Some(dir) = initial_dir
            && let Some(item) = create_shell_item(dir)
        {
            dialog.SetFolder(&item)?;
        }

        match dialog.Show(Some(hwnd)) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == 0x800704C7 => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// 保存先ダイアログ（IFileSaveDialog）
/// `initial_dir`が指定されていればそのフォルダを初期表示する
/// `title`/`ok_button_label`でダイアログのタイトルとOKボタンのラベルをカスタマイズ可能
pub fn save_file_dialog(
    hwnd: HWND,
    default_name: &str,
    filter_name: &str,
    filter_ext: &str,
    initial_dir: Option<&Path>,
    title: Option<&str>,
    ok_button_label: Option<&str>,
) -> Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileSaveDialog = windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Shell::FileSaveDialog,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )
        .context("FileSaveDialog作成失敗")?;

        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT)?;

        // タイトル・OKボタンラベルのカスタマイズ
        if let Some(t) = title {
            let wide = to_wide(t);
            dialog.SetTitle(windows::core::PCWSTR(wide.as_ptr()))?;
        }
        if let Some(label) = ok_button_label {
            let wide = to_wide(label);
            dialog.SetOkButtonLabel(windows::core::PCWSTR(wide.as_ptr()))?;
        }

        // 初期ディレクトリ設定
        if let Some(dir) = initial_dir
            && let Some(item) = create_shell_item(dir)
        {
            dialog.SetFolder(&item)?;
        }

        // フィルタ設定
        let fname = to_wide(filter_name);
        let fspec = to_wide(filter_ext);
        let filters = [windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
            pszName: windows::core::PCWSTR(fname.as_ptr()),
            pszSpec: windows::core::PCWSTR(fspec.as_ptr()),
        }];
        dialog.SetFileTypes(&filters)?;

        // デフォルトファイル名
        let name_wide = to_wide(default_name);
        dialog.SetFileName(windows::core::PCWSTR(name_wide.as_ptr()))?;

        match dialog.Show(Some(hwnd)) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == 0x800704C7 => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// ブックマーク読み込みダイアログ（.gvbmフィルタ + bookmarksフォルダ初期表示）
pub fn open_bookmark_dialog(hwnd: HWND) -> Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Shell::FileOpenDialog,
            None,
            windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
        )
        .context("FileOpenDialog作成失敗")?;

        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST)?;

        // 初期ディレクトリ: bookmarksフォルダ
        let bookmark_dir = crate::bookmark::bookmark_dir();
        let _ = std::fs::create_dir_all(&bookmark_dir);
        if let Some(item) = create_shell_item(&bookmark_dir) {
            dialog.SetFolder(&item)?;
        }

        // フィルタ: ブックマーク + すべてのファイル
        let filter_name: Vec<u16> = "ぐらびゅブックマーク\0".encode_utf16().collect();
        let filter_spec: Vec<u16> = "*.gvbm;*.gv3bm\0".encode_utf16().collect();
        let all_name: Vec<u16> = "すべてのファイル\0".encode_utf16().collect();
        let all_spec: Vec<u16> = "*.*\0".encode_utf16().collect();

        let filters = [
            windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR(filter_name.as_ptr()),
                pszSpec: windows::core::PCWSTR(filter_spec.as_ptr()),
            },
            windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR(all_name.as_ptr()),
                pszSpec: windows::core::PCWSTR(all_spec.as_ptr()),
            },
        ];
        dialog.SetFileTypes(&filters)?;

        match dialog.Show(Some(hwnd)) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == 0x800704C7 => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// 単一ファイルの移動（リネーム対応）
/// SHFileOperationW (FO_MOVE) で pTo にファイルパスを指定し、移動+リネームに対応する
pub fn move_single_file(hwnd: HWND, src: &Path, dest: &Path) -> Result<bool> {
    let from = build_single_path_string(src);
    let to = build_single_path_string(dest);
    // FOF_MULTIDESTFILES: pToをディレクトリではなくファイルパスとして解釈させる
    sh_file_operation(
        hwnd,
        FO_MOVE,
        &from,
        Some(&to),
        (FOF_ALLOWUNDO.0 | FOF_MULTIDESTFILES.0) as u16,
        &format!(
            "ファイル移動に失敗しました\n  src: {}\n  dest: {}",
            src.display(),
            dest.display()
        ),
    )
}

/// SHCreateItemFromParsingNameでIShellItemを取得するヘルパー
fn create_shell_item(dir: &Path) -> Option<windows::Win32::UI::Shell::IShellItem> {
    unsafe {
        let dir_wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        windows::Win32::UI::Shell::SHCreateItemFromParsingName(
            windows::core::PCWSTR(dir_wide.as_ptr()),
            None,
        )
        .ok()
    }
}

/// SHFileOperationW用のダブルNUL終端パス文字列を構築する（複数パス対応）
fn build_multi_path_string(paths: &[&Path]) -> Vec<u16> {
    let mut result = Vec::new();
    for path in paths {
        let path = crate::util::strip_extended_length_prefix(path);
        result.extend(path.as_os_str().encode_wide());
        result.push(0); // 各パスの後にNUL
    }
    result.push(0); // 終端の追加NUL
    result
}

/// SHFileOperationW用のダブルNUL終端パス文字列を構築する（単一パス）
fn build_single_path_string(path: &Path) -> Vec<u16> {
    let path = crate::util::strip_extended_length_prefix(path);
    let mut result: Vec<u16> = path.as_os_str().encode_wide().collect();
    result.push(0);
    result.push(0);
    result
}

use std::os::windows::ffi::OsStrExt;
