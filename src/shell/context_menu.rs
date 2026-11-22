use anyhow::Result;
use windows::Win32::System::Registry::*;

const FILE_MENU_KEY: &str = r"Software\Classes\*\shell\gv";
const DIR_MENU_KEY: &str = r"Software\Classes\Directory\shell\gv";

const OLD_FILE_MENU_KEY: &str = r"Software\Classes\*\shell\gv3";
const OLD_DIR_MENU_KEY: &str = r"Software\Classes\Directory\shell\gv3";

const DISPLAY_NAME: &str = "ぐらびゅで開く(&G)";

fn register_menu_key(menu_key: &str, exe_str: &str) -> Result<()> {
    super::association::set_key_value(HKEY_CURRENT_USER, menu_key, DISPLAY_NAME)?;

    let cmd_key = format!(r"{menu_key}\command");
    super::association::set_key_value(
        HKEY_CURRENT_USER,
        &cmd_key,
        &format!("\"{exe_str}\" \"%1\""),
    )?;

    Ok(())
}

pub fn register() -> Result<()> {
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_FILE_MENU_KEY);
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_DIR_MENU_KEY);

    let exe = crate::paths::exe_path()?;
    let exe_str = exe.to_string_lossy();

    register_menu_key(FILE_MENU_KEY, &exe_str)?;
    register_menu_key(DIR_MENU_KEY, &exe_str)?;

    Ok(())
}

pub fn unregister() -> Result<()> {
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_FILE_MENU_KEY);
    let _ = super::association::delete_key_tree(HKEY_CURRENT_USER, OLD_DIR_MENU_KEY);

    super::association::delete_key_tree(HKEY_CURRENT_USER, FILE_MENU_KEY)?;
    super::association::delete_key_tree(HKEY_CURRENT_USER, DIR_MENU_KEY)?;
    Ok(())
}
