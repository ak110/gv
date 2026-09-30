//! Win32 Shell APIによるファイル操作 + ダイアログ

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, IFileOpenDialog, IFileSaveDialog,
};

use crate::util::to_wide;

/// IFileOperation がキャンセルされたことを示す HRESULT (HRESULT_FROM_WIN32(ERROR_CANCELLED))
const ERROR_CANCELLED_HRESULT: u32 = 0x800704C7;

/// 内容を完成させてから保存先へ反映する。書き込み・置換に失敗しても既存内容を残す。
///
/// 同一フォルダで作成するため、置換が別ボリュームへのコピーにならない。
/// Windowsではstd::fs::renameが既存ファイルの置換を行い、共有違反・読み取り専用なら失敗する。
pub fn save_atomic(
    path: &Path,
    write_contents: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    let mut pending = PendingSave::create(path)?;
    let mut file = pending.file.take().expect("新規保存ファイルが存在する");
    let result = write_contents(&mut file).and_then(|()| file.sync_all().map_err(Into::into));
    // Windowsでの後始末・置換の前に必ずハンドルを閉じる。
    drop(file);
    result?;
    std::fs::rename(&pending.path, path)
        .with_context(|| format!("保存先を置換できませんでした: {}", path.display()))
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    save_atomic(path, |file| file.write_all(data).map_err(Into::into))
}

pub fn copy_atomic(source: &Path, destination: &Path) -> Result<()> {
    let mut source_file = File::open(source)
        .with_context(|| format!("コピー元を開けませんでした: {}", source.display()))?;
    save_atomic(destination, |file| {
        std::io::copy(&mut source_file, file)?;
        file.set_permissions(source_file.metadata()?.permissions())?;
        Ok(())
    })
}

/// 保存先へ反映するまでの新規ファイルだけを所有し、失敗時も回収する。
struct PendingSave {
    path: PathBuf,
    file: Option<File>,
}

impl PendingSave {
    fn create(destination: &Path) -> Result<Self> {
        static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let directory = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        loop {
            let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".gv-save-{}-{sequence}.tmp", std::process::id()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "保存用ファイルを作成できませんでした: {}",
                            directory.display()
                        )
                    });
                }
            }
        }
    }
}

impl Drop for PendingSave {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// IFileOperationによるファイル削除 (ごみ箱経由)
pub fn delete_to_recycle_bin(hwnd: HWND, paths: &[&Path]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO | FOFLAG_WANTNUKEWARNING)?;
        for path in paths {
            let item = create_shell_item_strict(path)?;
            op.DeleteItem(&item, None)
                .context("DeleteItemの設定に失敗しました")?;
        }
        perform_operations(&op, "ファイル削除に失敗しました")
    }
}

/// IFileOperationによるファイル移動 (複数→ディレクトリ)
pub fn move_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO)?;
        let dest_folder = create_shell_item_strict(dest)?;
        for path in paths {
            let item = create_shell_item_strict(path)?;
            op.MoveItem(&item, &dest_folder, None, None)
                .context("MoveItemの設定に失敗しました")?;
        }
        perform_operations(
            &op,
            &format!("ファイル移動に失敗しました\n  dest: {}", dest.display()),
        )
    }
}

/// IFileOperationによるファイルコピー(複数→ディレクトリ)
pub fn copy_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO)?;
        let dest_folder = create_shell_item_strict(dest)?;
        for path in paths {
            let item = create_shell_item_strict(path)?;
            op.CopyItem(&item, &dest_folder, None, None)
                .context("CopyItemの設定に失敗しました")?;
        }
        perform_operations(&op, "ファイルコピーに失敗しました")
    }
}

/// 単一ファイルの移動 (リネーム対応)
/// 移動先の親ディレクトリをIShellItemにし、ファイル名をMoveItemの引数で指定する
pub fn move_single_file(hwnd: HWND, src: &Path, dest: &Path) -> Result<bool> {
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO)?;
        let src_item = create_shell_item_strict(src)?;
        let dest_parent = dest
            .parent()
            .context("移動先の親ディレクトリが存在しない")?;
        let dest_folder = create_shell_item_strict(dest_parent)?;
        let new_name = dest.file_name().context("移動先のファイル名が存在しない")?;
        let new_name_wide: Vec<u16> = new_name.encode_wide().chain(std::iter::once(0)).collect();
        op.MoveItem(
            &src_item,
            &dest_folder,
            windows::core::PCWSTR(new_name_wide.as_ptr()),
            None,
        )
        .context("MoveItemの設定に失敗しました")?;
        perform_operations(
            &op,
            &format!(
                "ファイル移動に失敗しました\n  src: {}\n  dest: {}",
                src.display(),
                dest.display()
            ),
        )
    }
}

/// ファイル選択ダイアログ (IFileOpenDialog)
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
            Err(e) if e.code().0 as u32 == ERROR_CANCELLED_HRESULT => return Ok(None), // ユーザーキャンセル
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// フォルダ選択ダイアログ (IFileOpenDialog + FOS_PICKFOLDERS)
pub fn open_folder_dialog(hwnd: HWND, initial_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    select_folder_dialog(hwnd, "フォルダを開く", initial_dir)
}

/// フォルダ選択ダイアログ (移動/コピー先選択用)
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
            Err(e) if e.code().0 as u32 == ERROR_CANCELLED_HRESULT => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// `save_file_dialog` の設定パラメータ。
///
/// - `default_name`: デフォルトファイル名 (拡張子付きを推奨)。
/// - `filter_name` / `filter_ext`: ファイル種別フィルタの表示名と spec (`"*.png"` 等)。
/// - `default_ext`: 拡張子文字列 (`"png"` のように先頭ドット無し)。ユーザーがファイル名
///   から拡張子を削除した場合に Windows 側で自動補完される。補完が不要な場合 (`*.*`
///   フィルタなど) は空文字を渡す。
/// - `initial_dir`: 指定すればそのフォルダを初期表示する。
/// - `title` / `ok_button_label`: ダイアログのタイトルと OK ボタンラベルのカスタマイズ。
#[derive(Default)]
pub struct SaveFileDialogParams<'a> {
    pub default_name: &'a str,
    pub filter_name: &'a str,
    pub filter_ext: &'a str,
    pub default_ext: &'a str,
    pub initial_dir: Option<&'a Path>,
    pub title: Option<&'a str>,
    pub ok_button_label: Option<&'a str>,
}

/// 保存先ダイアログ (`IFileSaveDialog`) を表示してユーザーにパスを選択させる。
pub fn save_file_dialog(hwnd: HWND, params: SaveFileDialogParams<'_>) -> Result<Option<PathBuf>> {
    let SaveFileDialogParams {
        default_name,
        filter_name,
        filter_ext,
        default_ext,
        initial_dir,
        title,
        ok_button_label,
    } = params;
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

        // デフォルト拡張子の補完設定 (ユーザーがファイル名から拡張子を削除した場合に Windows
        // 側で自動補完される)。先頭ドット無しの拡張子文字列を渡す必要がある。
        let ext_wide;
        if !default_ext.is_empty() {
            ext_wide = to_wide(default_ext);
            dialog.SetDefaultExtension(windows::core::PCWSTR(ext_wide.as_ptr()))?;
        }

        // デフォルトファイル名
        let name_wide = to_wide(default_name);
        dialog.SetFileName(windows::core::PCWSTR(name_wide.as_ptr()))?;

        match dialog.Show(Some(hwnd)) {
            Ok(()) => {}
            Err(e) if e.code().0 as u32 == ERROR_CANCELLED_HRESULT => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

/// ブックマーク読み込みダイアログ (.gvbmフィルタ + bookmarksフォルダ初期表示)
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
        let filter_spec: Vec<u16> = "*.gvbm;*.gv3bm;*.gvb\0".encode_utf16().collect();
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
            Err(e) if e.code().0 as u32 == ERROR_CANCELLED_HRESULT => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let result = dialog.GetResult()?;
        let path_raw = result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(path_raw.to_string()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(path_raw.0 as *const _));
        Ok(Some(path))
    }
}

// --- IFileOperation ヘルパー ---

use windows::Win32::UI::Shell::FILEOPERATION_FLAGS;

/// IFileOperationのフラグ定数
const FOFLAG_ALLOWUNDO: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x0040);
const FOFLAG_WANTNUKEWARNING: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x4000);

/// IFileOperationを作成し、親ウィンドウとフラグを設定する
unsafe fn create_file_operation(
    hwnd: HWND,
    flags: FILEOPERATION_FLAGS,
) -> Result<windows::Win32::UI::Shell::IFileOperation> {
    unsafe {
        let op: windows::Win32::UI::Shell::IFileOperation =
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::UI::Shell::FileOperation,
                None,
                windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
            )
            .context("IFileOperation作成失敗")?;
        op.SetOwnerWindow(hwnd)?;
        op.SetOperationFlags(flags)?;
        Ok(op)
    }
}

/// IFileOperationの操作を実行し、結果を返す
/// 成功時はtrue、ユーザーキャンセル時はfalseを返す
unsafe fn perform_operations(
    op: &windows::Win32::UI::Shell::IFileOperation,
    error_msg: &str,
) -> Result<bool> {
    unsafe {
        op.PerformOperations().context(error_msg.to_string())?;
        let aborted = op.GetAnyOperationsAborted()?;
        Ok(!aborted.as_bool())
    }
}

/// SHCreateItemFromParsingNameでIShellItemを取得する (エラー時はResult)
/// ファイル操作用: 対象パスが存在しない場合はエラーとして扱う
fn create_shell_item_strict(path: &Path) -> Result<windows::Win32::UI::Shell::IShellItem> {
    let path = crate::util::strip_extended_length_prefix(path);
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        windows::Win32::UI::Shell::SHCreateItemFromParsingName(
            windows::core::PCWSTR(wide.as_ptr()),
            None,
        )
        .with_context(|| format!("ShellItem作成失敗: {}", path.display()))
    }
}

/// SHCreateItemFromParsingNameでIShellItemを取得するヘルパー
/// ダイアログの初期フォルダ設定用: 失敗時はNoneを返す
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;

    /// 書き込み途中と置換の失敗は元の内容を残し、失敗後も上書きと複製を実行できる。
    #[test]
    fn atomic_save_preserves_existing_content_on_failure_and_recovers() {
        let dir = crate::app::test_support::unique_temp_dir("atomic_save");
        let destination = dir.join("保存先.dat");
        std::fs::write(&destination, b"original").unwrap();

        let result = save_atomic(&destination, |file| {
            file.write_all(b"partial")?;
            anyhow::bail!("書き込み途中の障害");
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");

        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&destination)
            .unwrap();
        assert!(write_atomic(&destination, b"replacement").is_err());
        drop(locked);
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        write_atomic(&destination, b"replacement").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"replacement");

        let copy = dir.join("複製.dat");
        copy_atomic(&destination, &copy).unwrap();
        copy_atomic(&copy, &copy).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        assert!(copy_atomic(&dir.join("missing"), &copy).is_err());
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        let original_permissions = std::fs::metadata(&destination).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&destination, readonly_permissions).unwrap();
        let locked_copy = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&copy)
            .unwrap();
        assert!(copy_atomic(&destination, &copy).is_err());
        drop(locked_copy);
        std::fs::set_permissions(&destination, original_permissions).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        let mut expected = vec![destination.file_name().unwrap(), copy.file_name().unwrap()];
        expected.sort();
        assert_eq!(names, expected);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
