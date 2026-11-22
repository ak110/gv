use anyhow::Result;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

fn exe_path() -> Result<String> {
    let path = crate::paths::exe_path()?;
    Ok(path.to_string_lossy().into_owned())
}

use crate::util::to_wide;

pub(super) fn delete_key_tree(hkey: HKEY, subkey: &str) -> Result<()> {
    let result = unsafe { RegDeleteTreeW(hkey, windows::core::PCWSTR(to_wide(subkey).as_ptr())) };
    if result != ERROR_SUCCESS && result != windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        anyhow::bail!("レジストリキー削除失敗: {subkey} (error: {result:?})");
    }
    Ok(())
}

pub(super) fn set_key_value(root: HKEY, subkey: &str, value: &str) -> Result<()> {
    let wide_key = to_wide(subkey);
    let wide_val = to_wide(value);

    let mut hkey = HKEY::default();
    let result = unsafe {
        RegCreateKeyW(
            root,
            windows::core::PCWSTR(wide_key.as_ptr()),
            std::ptr::from_mut(&mut hkey),
        )
    };
    if result != ERROR_SUCCESS {
        anyhow::bail!("レジストリキー作成失敗: {subkey}");
    }

    // SAFETY: wide_val は直上で作成した Vec<u16> でありこのブロック内で借用が有効。
    // from_raw_parts は wide_val.len() * 2 バイトを u8 スライスとして参照しており範囲内に収まる。
    let result = unsafe {
        RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                wide_val.as_ptr().cast::<u8>(),
                wide_val.len() * 2,
            )),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if result != ERROR_SUCCESS {
        anyhow::bail!("レジストリ値設定失敗: {subkey}");
    }
    Ok(())
}

fn add_open_with_progid(extension: &str, progid: &str) -> Result<()> {
    let subkey = format!(r"Software\Classes\{extension}\OpenWithProgids");
    let wide_key = to_wide(&subkey);
    let wide_progid = to_wide(progid);

    let mut hkey = HKEY::default();
    let result = unsafe {
        RegCreateKeyW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(wide_key.as_ptr()),
            std::ptr::from_mut(&mut hkey),
        )
    };
    if result != ERROR_SUCCESS {
        anyhow::bail!("OpenWithProgids キー作成失敗: {extension}");
    }

    let result = unsafe {
        RegSetValueExW(
            hkey,
            windows::core::PCWSTR(wide_progid.as_ptr()),
            None,
            REG_NONE,
            None,
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if result != ERROR_SUCCESS {
        anyhow::bail!("OpenWithProgids 値設定失敗: {extension} -> {progid}");
    }
    Ok(())
}

fn remove_open_with_progid(extension: &str, progid: &str) {
    let subkey = format!(r"Software\Classes\{extension}\OpenWithProgids");

    let mut hkey = HKEY::default();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(to_wide(&subkey).as_ptr()),
            None,
            KEY_SET_VALUE,
            std::ptr::from_mut(&mut hkey),
        )
    };
    if result != ERROR_SUCCESS {
        return;
    }

    let _ = unsafe { RegDeleteValueW(hkey, windows::core::PCWSTR(to_wide(progid).as_ptr())) };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
}

use crate::bookmark::{BOOKMARK_EXTENSION, LEGACY_BOOKMARK_EXTENSION, OLD_BOOKMARK_EXTENSION};
use crate::image::StandardDecoder;

const IMAGE_PROGID: &str = "gv.ImageFile";
const ARCHIVE_PROGID: &str = "gv.ArchiveFile";
const BOOKMARK_PROGID: &str = "gv.Bookmark";

const OLD_IMAGE_PROGID: &str = "gv3.ImageFile";
const OLD_ARCHIVE_PROGID: &str = "gv3.ArchiveFile";
const OLD_BOOKMARK_PROGID: &str = "gv3.Bookmark";

pub fn register() -> Result<()> {
    let _ = cleanup_old_progids();

    let exe = exe_path()?;

    let progid_key = format!(r"Software\Classes\{IMAGE_PROGID}");
    set_key_value(HKEY_CURRENT_USER, &progid_key, "ぐらびゅ 画像ファイル")?;
    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"{progid_key}\shell\open\command"),
        &format!("\"{exe}\" \"%1\""),
    )?;

    let progid_key = format!(r"Software\Classes\{ARCHIVE_PROGID}");
    set_key_value(
        HKEY_CURRENT_USER,
        &progid_key,
        "ぐらびゅ アーカイブファイル",
    )?;
    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"{progid_key}\shell\open\command"),
        &format!("\"{exe}\" \"%1\""),
    )?;

    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{IMAGE_PROGID}\DefaultIcon"),
        &format!("\"{exe}\",1"),
    )?;
    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{ARCHIVE_PROGID}\DefaultIcon"),
        &format!("\"{exe}\",2"),
    )?;

    let bm_key = format!(r"Software\Classes\{BOOKMARK_PROGID}");
    set_key_value(HKEY_CURRENT_USER, &bm_key, "ぐらびゅ ブックマーク")?;
    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"{bm_key}\shell\open\command"),
        &format!("\"{exe}\" \"%1\""),
    )?;
    set_key_value(
        HKEY_CURRENT_USER,
        &format!(r"{bm_key}\DefaultIcon"),
        &format!("\"{exe}\",2"),
    )?;

    for ext in crate::archive::generic_extensions() {
        remove_open_with_progid(ext, ARCHIVE_PROGID);
    }

    for ext in StandardDecoder::extensions() {
        add_open_with_progid(ext, IMAGE_PROGID)?;
    }
    for ext in crate::archive::comic_extensions() {
        add_open_with_progid(ext, ARCHIVE_PROGID)?;
    }
    add_open_with_progid(BOOKMARK_EXTENSION, BOOKMARK_PROGID)?;
    add_open_with_progid(OLD_BOOKMARK_EXTENSION, BOOKMARK_PROGID)?;
    add_open_with_progid(LEGACY_BOOKMARK_EXTENSION, BOOKMARK_PROGID)?;

    Ok(())
}

fn cleanup_old_progids() -> Result<()> {
    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{OLD_IMAGE_PROGID}"),
    )?;
    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{OLD_ARCHIVE_PROGID}"),
    )?;
    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{OLD_BOOKMARK_PROGID}"),
    )?;

    for ext in StandardDecoder::extensions() {
        remove_open_with_progid(ext, OLD_IMAGE_PROGID);
    }
    for ext in crate::archive::comic_extensions() {
        remove_open_with_progid(ext, OLD_ARCHIVE_PROGID);
    }
    for ext in crate::archive::generic_extensions() {
        remove_open_with_progid(ext, OLD_ARCHIVE_PROGID);
    }
    remove_open_with_progid(OLD_BOOKMARK_EXTENSION, OLD_BOOKMARK_PROGID);
    Ok(())
}

pub fn unregister() -> Result<()> {
    let _ = cleanup_old_progids();

    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{IMAGE_PROGID}"),
    )?;
    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{ARCHIVE_PROGID}"),
    )?;
    delete_key_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\{BOOKMARK_PROGID}"),
    )?;

    for ext in StandardDecoder::extensions() {
        remove_open_with_progid(ext, IMAGE_PROGID);
    }
    for ext in crate::archive::comic_extensions() {
        remove_open_with_progid(ext, ARCHIVE_PROGID);
    }
    for ext in crate::archive::generic_extensions() {
        remove_open_with_progid(ext, ARCHIVE_PROGID);
    }
    remove_open_with_progid(BOOKMARK_EXTENSION, BOOKMARK_PROGID);
    remove_open_with_progid(OLD_BOOKMARK_EXTENSION, BOOKMARK_PROGID);
    remove_open_with_progid(LEGACY_BOOKMARK_EXTENSION, BOOKMARK_PROGID);

    Ok(())
}

pub fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_progid_constants_use_gv3_prefix() {
        assert_eq!(OLD_IMAGE_PROGID, "gv3.ImageFile");
        assert_eq!(OLD_ARCHIVE_PROGID, "gv3.ArchiveFile");
        assert_eq!(OLD_BOOKMARK_PROGID, "gv3.Bookmark");
    }

    #[test]
    fn new_and_old_progids_differ() {
        for current in [IMAGE_PROGID, ARCHIVE_PROGID, BOOKMARK_PROGID] {
            for old in [OLD_IMAGE_PROGID, OLD_ARCHIVE_PROGID, OLD_BOOKMARK_PROGID] {
                assert!(
                    !current.eq_ignore_ascii_case(old),
                    "{current} conflicts with {old}"
                );
            }
        }
    }
}
