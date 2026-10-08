//! 実行ファイルと隣接する設定・資源の配置。

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

pub fn exe_path() -> Result<PathBuf> {
    std::env::current_exe().context("実行ファイルのパスを取得できませんでした")
}

pub fn exe_dir() -> Result<PathBuf> {
    let exe = exe_path()?;
    exe.parent()
        .map(Path::to_path_buf)
        .with_context(|| format!("実行ファイルの親フォルダーがありません: {}", exe.display()))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(exe_dir()?.join("ぐらびゅ.toml"))
}

pub fn key_config_path() -> Result<PathBuf> {
    Ok(exe_dir()?.join("ぐらびゅ.keys.toml"))
}

pub fn bookmark_dir() -> PathBuf {
    exe_dir().map_or_else(|_| PathBuf::from("bookmarks"), |dir| dir.join("bookmarks"))
}

pub fn susie_plugin_dir(configured: &str) -> Result<PathBuf> {
    Ok(resolve_plugin_dir(&exe_dir()?, configured))
}

fn resolve_plugin_dir(exe_dir: &Path, configured: &str) -> PathBuf {
    exe_dir.join(configured)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_resources_use_executable_directory() {
        let dir = exe_path().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(exe_dir().unwrap(), dir);
        assert_eq!(config_path().unwrap(), dir.join("ぐらびゅ.toml"));
        assert_eq!(key_config_path().unwrap(), dir.join("ぐらびゅ.keys.toml"));
        assert_eq!(bookmark_dir(), dir.join("bookmarks"));
        assert_eq!(
            susie_plugin_dir("custom-spi").unwrap(),
            dir.join("custom-spi")
        );
    }

    #[test]
    fn plugin_directory_accepts_relative_and_absolute_paths() {
        let dir = exe_dir().unwrap();
        assert_eq!(
            resolve_plugin_dir(&dir, "custom-spi"),
            dir.join("custom-spi")
        );
        let absolute = dir.join("absolute-spi");
        assert_eq!(
            resolve_plugin_dir(&dir, absolute.to_str().unwrap()),
            absolute
        );
    }
}
