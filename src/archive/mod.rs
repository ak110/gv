mod rar;
mod sevenz;
mod zip;

use std::path::Path;

use anyhow::{Result, bail};

/// アーカイブ対応拡張子（小文字、ドット付き）
const ARCHIVE_EXTENSIONS: &[&str] = &[".zip", ".cbz", ".rar", ".cbr", ".7z"];

/// アーカイブハンドラのトレイト
/// 各フォーマットのハンドラが実装する
pub trait ArchiveHandler: Send + Sync {
    fn supported_extensions(&self) -> &[&str];

    /// アーカイブ内の画像ファイルをtarget_dirに展開する
    /// 戻り値: 展開されたファイル数
    fn extract_images(&self, archive_path: &Path, target_dir: &Path) -> Result<usize>;
}

/// アーカイブハンドラのディスパッチャ
pub struct ArchiveManager {
    handlers: Vec<Box<dyn ArchiveHandler>>,
}

impl ArchiveManager {
    pub fn new() -> Self {
        Self {
            handlers: vec![
                Box::new(zip::ZipHandler),
                Box::new(rar::RarHandler),
                Box::new(sevenz::SevenZHandler),
            ],
        }
    }

    /// パスがアーカイブファイルか拡張子で判定する
    pub fn is_archive(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let ext = format!(".{}", e.to_lowercase());
                ARCHIVE_EXTENSIONS.contains(&ext.as_str())
            })
            .unwrap_or(false)
    }

    /// 対応ハンドラでアーカイブ内の画像を一括展開する
    pub fn extract_images(&self, archive_path: &Path, target_dir: &Path) -> Result<usize> {
        let ext = archive_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        for handler in &self.handlers {
            if handler.supported_extensions().contains(&ext.as_str()) {
                return handler.extract_images(archive_path, target_dir);
            }
        }

        bail!("未対応のアーカイブ形式: {}", archive_path.display());
    }
}

/// アーカイブ展開時のファイル名重複を解決する
/// target_dirに同名ファイルが既に存在する場合、"_2", "_3"...のサフィックスを付ける
pub fn resolve_filename(target_dir: &Path, original_name: &str) -> std::path::PathBuf {
    let path = target_dir.join(original_name);
    if !path.exists() {
        return path;
    }

    let stem = Path::new(original_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(original_name);
    let ext = Path::new(original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    for i in 2..=9999 {
        let new_name = if ext.is_empty() {
            format!("{stem}_{i}")
        } else {
            format!("{stem}_{i}.{ext}")
        };
        let new_path = target_dir.join(&new_name);
        if !new_path.exists() {
            return new_path;
        }
    }

    // 万が一9999まで使い切った場合（実質ありえない）
    target_dir.join(format!("{original_name}_overflow"))
}

/// アーカイブエントリのパスからファイル名部分のみを抽出する
/// アーカイブ内のサブフォルダ構造はフラット化する
pub fn extract_filename(entry_path: &str) -> &str {
    // パス区切りは '/' と '\' の両方を考慮
    entry_path.rsplit(['/', '\\']).next().unwrap_or(entry_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_archive_recognizes_extensions() {
        assert!(ArchiveManager::is_archive(Path::new("test.zip")));
        assert!(ArchiveManager::is_archive(Path::new("test.ZIP")));
        assert!(ArchiveManager::is_archive(Path::new("test.cbz")));
        assert!(ArchiveManager::is_archive(Path::new("test.rar")));
        assert!(ArchiveManager::is_archive(Path::new("test.cbr")));
        assert!(ArchiveManager::is_archive(Path::new("test.7z")));
        assert!(!ArchiveManager::is_archive(Path::new("test.jpg")));
        assert!(!ArchiveManager::is_archive(Path::new("test.txt")));
        assert!(!ArchiveManager::is_archive(Path::new("test")));
    }

    #[test]
    fn extract_filename_flattens_paths() {
        assert_eq!(extract_filename("folder/subfolder/image.jpg"), "image.jpg");
        assert_eq!(extract_filename("folder\\image.png"), "image.png");
        assert_eq!(extract_filename("image.bmp"), "image.bmp");
        assert_eq!(extract_filename(""), "");
    }

    #[test]
    fn resolve_filename_handles_duplicates() {
        let dir = std::env::temp_dir().join("gv3_test_resolve_fn");
        let _ = std::fs::create_dir_all(&dir);

        // 重複なし
        let path = resolve_filename(&dir, "unique.jpg");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "unique.jpg");

        // 重複あり
        std::fs::write(dir.join("dup.jpg"), b"").unwrap();
        let path = resolve_filename(&dir, "dup.jpg");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "dup_2.jpg");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
