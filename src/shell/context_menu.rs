//! エクスプローラのコンテキストメニュー（右クリックメニュー）への登録
//!
//! - HKCU\Software\Classes\*\shell\gv3 — 全ファイル対象
//! - HKCU\Software\Classes\Directory\shell\gv3 — フォルダ対象

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;

/// 全ファイル対象のメニューキー
const FILE_MENU_KEY: &str = r"Software\Classes\*\shell\gv3";
/// フォルダ対象のメニューキー
const DIR_MENU_KEY: &str = r"Software\Classes\Directory\shell\gv3";

/// 表示名（アクセスキー G）
const DISPLAY_NAME: &str = "ぐらびゅ3で開く(&G)";

/// ワイド文字列（null終端）を作成するヘルパー
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 指定キーにメニュー項目を登録する
fn register_menu_key(menu_key: &str, exe_str: &str) -> Result<()> {
    // メニュー項目キーを作成し、表示名を設定
    let mut hkey = HKEY::default();
    let result = unsafe {
        RegCreateKeyW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(to_wide(menu_key).as_ptr()),
            &mut hkey,
        )
    };
    if result != ERROR_SUCCESS {
        anyhow::bail!("コンテキストメニュー キー作成失敗: {menu_key}");
    }

    let display_name = to_wide(DISPLAY_NAME);
    let result = unsafe {
        RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                display_name.as_ptr() as *const u8,
                display_name.len() * 2,
            )),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if result != ERROR_SUCCESS {
        anyhow::bail!("コンテキストメニュー 表示名設定失敗: {menu_key}");
    }

    // command サブキー
    let cmd_key = format!(r"{menu_key}\command");
    let mut hkey = HKEY::default();
    let result = unsafe {
        RegCreateKeyW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(to_wide(&cmd_key).as_ptr()),
            &mut hkey,
        )
    };
    if result != ERROR_SUCCESS {
        anyhow::bail!("コンテキストメニュー command キー作成失敗: {menu_key}");
    }

    let command = to_wide(&format!("\"{exe_str}\" \"%1\""));
    let result = unsafe {
        RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                command.as_ptr() as *const u8,
                command.len() * 2,
            )),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if result != ERROR_SUCCESS {
        anyhow::bail!("コンテキストメニュー command 値設定失敗: {menu_key}");
    }

    Ok(())
}

/// 指定キーのメニュー項目を削除する
fn unregister_menu_key(menu_key: &str) -> Result<()> {
    let result = unsafe {
        RegDeleteTreeW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(to_wide(menu_key).as_ptr()),
        )
    };
    if result != ERROR_SUCCESS && result != windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        anyhow::bail!("コンテキストメニュー キー削除失敗: {menu_key} (error: {result:?})");
    }
    Ok(())
}

/// コンテキストメニューを登録する（ファイル + フォルダ）
pub fn register() -> Result<()> {
    let exe = std::env::current_exe().context("exe パス取得失敗")?;
    let exe_str = exe.to_string_lossy();

    register_menu_key(FILE_MENU_KEY, &exe_str)?;
    register_menu_key(DIR_MENU_KEY, &exe_str)?;

    Ok(())
}

/// コンテキストメニューを解除する（ファイル + フォルダ）
pub fn unregister() -> Result<()> {
    unregister_menu_key(FILE_MENU_KEY)?;
    unregister_menu_key(DIR_MENU_KEY)?;
    Ok(())
}
