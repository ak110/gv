//! ファイル・フォルダ・ブックマークのコモンダイアログ

use crate::util::to_wide;
use anyhow::{Context as _, Result};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, IFileOpenDialog, IFileSaveDialog,
};

/// HRESULT_FROM_WIN32(ERROR_CANCELLED)
const ERROR_CANCELLED_HRESULT: u32 = 0x800704C7;

fn extension_filter_spec<'a>(extensions: impl IntoIterator<Item = &'a str>) -> String {
    extensions
        .into_iter()
        .map(|extension| format!("*{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// ロード済みプラグインを含む対応形式から、表示順にファイル種類を作成する。
fn open_file_filters(
    registry: &crate::extension_registry::ExtensionRegistry,
) -> Vec<(&'static str, String)> {
    let images = registry.image_extensions();
    let mut containers = registry.archive_extensions();
    containers.push(crate::extension_registry::PDF_EXTENSION);
    let bookmarks = crate::bookmark::BOOKMARK_EXTENSIONS;
    let supported = images
        .iter()
        .copied()
        .chain(containers.iter().copied())
        .chain(bookmarks.iter().copied());
    vec![
        ("対応ファイル", extension_filter_spec(supported)),
        ("画像", extension_filter_spec(images)),
        ("アーカイブ・PDF", extension_filter_spec(containers)),
        (
            "ブックマーク",
            extension_filter_spec(bookmarks.iter().copied()),
        ),
        ("すべてのファイル", "*.*".to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_types_include_registered_extensions_and_bookmarks() {
        let mut registry = crate::extension_registry::ExtensionRegistry::new();
        registry.register_image_extensions(&[".psd".to_string()]);
        registry.register_archive_extensions(&[".lzh".to_string()]);
        let filters = open_file_filters(&registry);
        assert_eq!(
            filters.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            [
                "対応ファイル",
                "画像",
                "アーカイブ・PDF",
                "ブックマーク",
                "すべてのファイル"
            ]
        );
        let expected = registry
            .image_extensions()
            .into_iter()
            .chain(registry.archive_extensions())
            .chain([crate::extension_registry::PDF_EXTENSION])
            .chain(crate::bookmark::BOOKMARK_EXTENSIONS.iter().copied());
        let supported: Vec<_> = filters[0].1.split(';').collect();
        for extension in expected {
            assert!(
                supported.contains(&format!("*{extension}").as_str()),
                "{extension}"
            );
        }
        assert!(filters[1].1.split(';').any(|spec| spec == "*.psd"));
        assert!(filters[2].1.split(';').any(|spec| spec == "*.lzh"));
    }
}

/// ファイル選択ダイアログ (IFileOpenDialog)
pub fn open_file_dialog(
    hwnd: HWND,
    initial_dir: Option<&Path>,
    registry: &crate::extension_registry::ExtensionRegistry,
) -> Result<Option<PathBuf>> {
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

        let types = open_file_filters(registry);
        let names: Vec<_> = types.iter().map(|(name, _)| to_wide(name)).collect();
        let specs: Vec<_> = types.iter().map(|(_, spec)| to_wide(spec)).collect();
        // SAFETY: namesとspecsをSetFileTypesの完了まで保持し、各PCWSTRは末尾NUL付きの対応するVecを指す。
        let filters: Vec<_> = names
            .iter()
            .zip(&specs)
            .map(
                |(name, spec)| windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                    pszName: windows::core::PCWSTR(name.as_ptr()),
                    pszSpec: windows::core::PCWSTR(spec.as_ptr()),
                },
            )
            .collect();
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
    #[cfg(test)]
    if let Some(path) = crate::shell::file_operations::test_driver::selected_path() {
        return Ok(Some(path));
    }
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
    #[cfg(test)]
    if let Some(path) = crate::shell::file_operations::test_driver::selected_path() {
        return Ok(Some(path));
    }
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
        let filter_name = to_wide("ぐらびゅブックマーク");
        let filter_spec = to_wide(&extension_filter_spec(
            crate::bookmark::BOOKMARK_EXTENSIONS.iter().copied(),
        ));
        let all_name = to_wide("すべてのファイル");
        let all_spec = to_wide("*.*");

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
