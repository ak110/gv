//! エクスプローラのコンテキストメニュー（右クリックメニュー）への登録
//!
//! - HKCU\Software\Classes\*\shell\gv — 全ファイル対象
//! - HKCU\Software\Classes\Directory\shell\gv — フォルダ対象

use anyhow::{Context as _, Result};
use windows::Win32::System::Registry::*;

/// 全ファイル対象のメニューキー
const FILE_MENU_KEY: &str = r"Software\Classes\*\shell\gv";
/// フォルダ対象のメニューキー
const DIR_MENU_KEY: &str = r"Software\Classes\Directory\shell\gv";

// 旧メニューキー（マイグレーション用）
const OLD_FILE_MENU_KEY: &str = r"Software\Classes\*\shell\gv3";
const OLD_DIR_MENU_KEY: &str = r"Software\Classes\Directory\shell\gv3";

/// 表示名（アクセスキー G）
const DISPLAY_NAME: &str = "ぐらびゅで開く(&G)";

/// 指定キーにメニュー項目を登録する
fn register_menu_key(menu_key: &str, exe_str: &str) -> Result<()> {
    // メニュー項目キーを作成し、表示名を設定
    super::association::set_key_value(HKEY_CURRENT_USER, menu_key, DISPLAY_NAME)?;

    // command サブキー
    let cmd_key = format!(r"{menu_key}\command");
    super::association::set_key_value(
        HKEY_CURRENT_USER,
        &cmd_key,
        &format!("\"{exe_str}\" \"%1\""),
    )?;

    Ok(())
}

/// コンテキストメニューを登録する（ファイル + フォルダ）
pub fn register() -> Result<()> {
    // 旧メニューキーのクリーンアップ
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_FILE_MENU_KEY);
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_DIR_MENU_KEY);

    let exe = std::env::current_exe().context("exe パス取得失敗")?;
    let exe_str = exe.to_string_lossy();

    register_menu_key(FILE_MENU_KEY, &exe_str)?;
    register_menu_key(DIR_MENU_KEY, &exe_str)?;

    Ok(())
}

/// コンテキストメニューを解除する（ファイル + フォルダ）
pub fn unregister() -> Result<()> {
    // 旧メニューキーも削除
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_FILE_MENU_KEY);
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_DIR_MENU_KEY);

    super::association::delete_key_tree(HKEY_CURRENT_USER, FILE_MENU_KEY)?;
    super::association::delete_key_tree(HKEY_CURRENT_USER, DIR_MENU_KEY)?;
    Ok(())
}
