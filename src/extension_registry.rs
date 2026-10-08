use std::collections::HashSet;
use std::path::Path;

pub const PDF_EXTENSION: &str = ".pdf";

/// 画像・アーカイブ拡張子のレジストリ。
///
/// 起動時にデフォルト拡張子で初期化され、`Susie` プラグインがロードされた際に
/// `register_image_extensions` / `register_archive_extensions` を通じて追加登録される。
/// `is_image_extension` / `is_archive_extension` でファイル名が画像/アーカイブかを判定する。
pub struct ExtensionRegistry {
    image_extensions: HashSet<String>,
    archive_extensions: HashSet<String>,
}

impl ExtensionRegistry {
    /// デフォルト拡張子で初期化
    pub fn new() -> Self {
        let image_extensions = crate::image::StandardDecoder::extensions()
            .map(ToString::to_string)
            .collect();

        let archive_extensions = crate::archive::builtin_formats()
            .into_iter()
            .flat_map(|(_, extensions)| extensions.iter())
            .map(ToString::to_string)
            .collect();

        Self {
            image_extensions,
            archive_extensions,
        }
    }

    pub fn is_pdf_path(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(PDF_EXTENSION.trim_start_matches('.')))
    }

    pub fn image_extensions(&self) -> Vec<&str> {
        let mut extensions: Vec<_> = self.image_extensions.iter().map(String::as_str).collect();
        extensions.sort_unstable();
        extensions
    }

    pub fn archive_extensions(&self) -> Vec<&str> {
        let mut extensions: Vec<_> = self.archive_extensions.iter().map(String::as_str).collect();
        extensions.sort_unstable();
        extensions
    }

    /// ファイル名が画像拡張子を持つか判定する
    pub fn is_image_extension(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        self.image_extensions.iter().any(|ext| lower.ends_with(ext))
    }

    /// ファイル名がアーカイブ拡張子を持つか判定する
    pub fn is_archive_extension(&self, filename: &str) -> bool {
        let lower = filename.to_lowercase();
        self.archive_extensions
            .iter()
            .any(|ext| lower.ends_with(ext))
    }

    /// 画像拡張子を追加登録する (ドット付き小文字、例: ".psd")
    pub fn register_image_extensions(&mut self, exts: &[String]) {
        for ext in exts {
            self.image_extensions.insert(ext.clone());
        }
    }

    /// アーカイブ拡張子を追加登録する
    pub fn register_archive_extensions(&mut self, exts: &[String]) {
        for ext in exts {
            self.archive_extensions.insert(ext.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_image_extensions() {
        let reg = ExtensionRegistry::new();
        assert!(reg.is_image_extension("test.jpg"));
        assert!(reg.is_image_extension("test.PNG"));
        assert!(reg.is_image_extension("test.WebP"));
        assert!(!reg.is_image_extension("test.txt"));
    }

    #[test]
    fn default_archive_extensions() {
        let reg = ExtensionRegistry::new();
        assert!(reg.is_archive_extension("test.zip"));
        assert!(reg.is_archive_extension("test.RAR"));
        assert!(reg.is_archive_extension("test.7z"));
        assert!(!reg.is_archive_extension("test.jpg"));
    }

    #[test]
    fn register_custom_extensions() {
        let mut reg = ExtensionRegistry::new();
        reg.register_image_extensions(&[".psd".to_string(), ".xcf".to_string()]);
        assert!(reg.is_image_extension("photo.psd"));
        assert!(reg.is_image_extension("drawing.XCF"));

        reg.register_archive_extensions(&[".lzh".to_string()]);
        assert!(reg.is_archive_extension("archive.lzh"));
    }
}
