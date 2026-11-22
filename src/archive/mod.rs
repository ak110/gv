// Linuxからのクロスビルドは`--cfg gv_no_rar`でRAR展開を除外する（unrarのビルドがクロスビルドに対応しないため）。
#[cfg(not(gv_no_rar))]
mod rar;
mod sevenz;
pub mod susie;
pub mod zip;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::extension_registry::ExtensionRegistry;

pub fn builtin_formats() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("ZIP", zip::EXTENSIONS),
        #[cfg(not(gv_no_rar))]
        ("RAR", rar::EXTENSIONS),
        ("7z", sevenz::EXTENSIONS),
    ]
}

pub fn comic_extensions() -> Vec<&'static str> {
    vec![
        zip::EXTENSIONS[1],
        #[cfg(not(gv_no_rar))]
        rar::EXTENSIONS[1],
    ]
}

pub fn generic_extensions() -> Vec<&'static str> {
    vec![
        zip::EXTENSIONS[0],
        #[cfg(not(gv_no_rar))]
        rar::EXTENSIONS[0],
        sevenz::EXTENSIONS[0],
    ]
}

pub type ExtractedEntry = (PathBuf, String, std::time::SystemTime);

pub struct ArchiveImageEntry {
    pub entry_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub entry_index: u32,
    pub modified: std::time::SystemTime,
}

pub trait ArchiveHandler: Send + Sync {
    fn supported_extensions(&self) -> Vec<String>;

    fn extract_images(
        &self,
        _archive_path: &Path,
        _target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        bail!("一括展開未対応")
    }

    fn supports_on_demand(&self) -> bool {
        false
    }

    fn list_images_from_buffer(&self, _buffer: &[u8]) -> Result<Vec<ArchiveImageEntry>> {
        bail!("オンデマンド列挙未対応")
    }

    fn read_entry_at(&self, _path: &Path, _buffer: Option<&[u8]>, _index: u32) -> Result<Vec<u8>> {
        bail!("オンデマンド取得未対応")
    }
}

pub struct ArchiveManager {
    handlers: Vec<Box<dyn ArchiveHandler>>,
    registry: Arc<ExtensionRegistry>,
}

impl ArchiveManager {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self {
            handlers: vec![
                Box::new(zip::ZipHandler::new(Arc::clone(&registry))),
                #[cfg(not(gv_no_rar))]
                Box::new(rar::RarHandler::new(Arc::clone(&registry))),
                Box::new(sevenz::SevenZHandler::new(Arc::clone(&registry))),
            ],
            registry,
        }
    }

    pub fn add_handler(&mut self, handler: Box<dyn ArchiveHandler>) {
        self.handlers.push(handler);
    }

    fn normalized_extension(path: &Path) -> String {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default()
    }

    fn find_handler(&self, archive_path: &Path) -> Result<&dyn ArchiveHandler> {
        let ext = Self::normalized_extension(archive_path);
        self.handlers
            .iter()
            .find(|h| h.supported_extensions().contains(&ext))
            .map(AsRef::as_ref)
            .ok_or_else(|| anyhow::anyhow!("未対応のアーカイブ形式: {}", archive_path.display()))
    }

    pub fn is_archive(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| self.registry.is_archive_extension(name))
    }

    pub fn extract_images(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<Vec<ExtractedEntry>> {
        self.find_handler(archive_path)?
            .extract_images(archive_path, target_dir)
    }

    pub fn supports_on_demand(&self, archive_path: &Path) -> bool {
        self.find_handler(archive_path)
            .is_ok_and(ArchiveHandler::supports_on_demand)
    }

    pub fn list_images_from_buffer(
        &self,
        buffer: &[u8],
        archive_path: &Path,
    ) -> Result<Vec<ArchiveImageEntry>> {
        self.find_handler(archive_path)?
            .list_images_from_buffer(buffer)
    }

    pub fn read_entry_at(
        &self,
        archive_path: &Path,
        buffer: Option<&[u8]>,
        index: u32,
    ) -> Result<Vec<u8>> {
        self.find_handler(archive_path)?
            .read_entry_at(archive_path, buffer, index)
    }
}

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

    target_dir.join(format!("{original_name}_overflow"))
}

pub fn extract_filename(entry_path: &str) -> &str {
    entry_path.rsplit(['/', '\\']).next().unwrap_or(entry_path)
}

pub fn is_image_entry(path: &str, is_directory: bool, registry: &ExtensionRegistry) -> bool {
    let name = extract_filename(path);
    !is_directory && !name.is_empty() && !name.starts_with('.') && registry.is_image_extension(name)
}

pub fn dos_modified(packed: u32) -> std::time::SystemTime {
    let Ok(time) = ::zip::DateTime::try_from_msdos((packed >> 16) as u16, packed as u16) else {
        return std::time::SystemTime::UNIX_EPOCH;
    };
    let year = time.year();
    let leap = |year: u16| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let mut months = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if leap(year) {
        months[1] = 29;
    }
    if u64::from(time.day()) > months[usize::from(time.month() - 1)] {
        return std::time::SystemTime::UNIX_EPOCH;
    }
    let days = (1970..year)
        .map(|year| if leap(year) { 366u64 } else { 365 })
        .sum::<u64>()
        + months[..usize::from(time.month() - 1)].iter().sum::<u64>()
        + u64::from(time.day() - 1);
    let seconds = days * 86400
        + u64::from(time.hour()) * 3600
        + u64::from(time.minute()) * 60
        + u64::from(time.second());
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_archive_recognizes_extensions() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        assert!(mgr.is_archive(Path::new("test.zip")));
        assert!(mgr.is_archive(Path::new("test.ZIP")));
        assert!(mgr.is_archive(Path::new("test.cbz")));
        assert!(mgr.is_archive(Path::new("test.7z")));
        assert_eq!(mgr.is_archive(Path::new("test.rar")), cfg!(not(gv_no_rar)));
        assert_eq!(mgr.is_archive(Path::new("test.cbr")), cfg!(not(gv_no_rar)));
        assert!(!mgr.is_archive(Path::new("test.jpg")));
        assert!(!mgr.is_archive(Path::new("test.txt")));
        assert!(!mgr.is_archive(Path::new("test")));
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
        let dir = crate::test_helpers::TempDir::new("resolve_fn");
        assert_eq!(resolve_filename(&dir, "unique.jpg"), dir.join("unique.jpg"));
        std::fs::write(dir.join("dup.jpg"), b"first").unwrap();
        assert_eq!(resolve_filename(&dir, "dup.jpg"), dir.join("dup_2.jpg"));
        assert_eq!(std::fs::read(dir.join("dup.jpg")).unwrap(), b"first");
    }

    #[test]
    fn find_handler_returns_error_for_unsupported_extension() {
        let mgr = ArchiveManager::new(Arc::new(ExtensionRegistry::new()));
        match mgr.find_handler(Path::new("test.xyz")) {
            Err(err) => assert_eq!(err.to_string(), "未対応のアーカイブ形式: test.xyz"),
            Ok(_) => panic!("未対応の拡張子が選択された"),
        }
    }

    #[test]
    fn find_handler_returns_error_for_no_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        let result = mgr.find_handler(Path::new("noext"));
        assert!(result.is_err());
    }

    #[test]
    fn find_handler_succeeds_for_supported_extensions() {
        let mgr = ArchiveManager::new(Arc::new(ExtensionRegistry::new()));
        let buf = crate::test_helpers::create_test_zip_buffer(&[("a.png", b"png")]);
        for name in ["a.zip", "a.cbz", "a.ZIP"] {
            let entries = mgr.list_images_from_buffer(&buf, Path::new(name)).unwrap();
            assert_eq!(entries.len(), 1, "{name}");
            assert_eq!(entries[0].file_name, "a.png", "{name}");
        }
        let sevenz = mgr
            .find_handler(Path::new("a.7z"))
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(sevenz.supported_extensions(), [".7z"]);
        for name in ["a.rar", "a.cbr", "a.Rar"] {
            let result = mgr.find_handler(Path::new(name));
            #[cfg(not(gv_no_rar))]
            assert_eq!(
                result
                    .unwrap_or_else(|err| panic!("{err}"))
                    .supported_extensions(),
                [".rar", ".cbr"]
            );
            #[cfg(gv_no_rar)]
            assert!(result.is_err(), "{name}");
        }
    }

    #[test]
    fn list_images_from_buffer_empty_zip() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        let buf = {
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let writer = ::zip::ZipWriter::new(cursor);
            writer.finish().unwrap();
            v
        };
        let entries = mgr
            .list_images_from_buffer(&buf, Path::new("empty.zip"))
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_images_from_buffer_filters_non_images() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        let buf = {
            use std::io::Write;
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let mut writer = ::zip::ZipWriter::new(cursor);
            let opts = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Stored);
            writer.start_file("photo.jpg", opts).unwrap();
            writer.write_all(b"jpeg-data").unwrap();
            writer.start_file("readme.txt", opts).unwrap();
            writer.write_all(b"text").unwrap();
            writer.start_file("doc.pdf", opts).unwrap();
            writer.write_all(b"pdf").unwrap();
            writer.start_file("icon.png", opts).unwrap();
            writer.write_all(b"png-data").unwrap();
            writer.start_file("data.xml", opts).unwrap();
            writer.write_all(b"xml").unwrap();
            writer.finish().unwrap();
            v
        };
        let entries = mgr
            .list_images_from_buffer(&buf, Path::new("mixed.zip"))
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name, "photo.jpg");
        assert_eq!(entries[1].file_name, "icon.png");
    }

    #[test]
    fn list_images_from_buffer_works_with_cbz_extension() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);

        let buf = {
            use std::io::Write;
            let mut v = Vec::new();
            let cursor = std::io::Cursor::new(&mut v);
            let mut writer = ::zip::ZipWriter::new(cursor);
            let opts = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Stored);
            writer.start_file("page1.png", opts).unwrap();
            writer.write_all(b"png").unwrap();
            writer.finish().unwrap();
            v
        };
        let path = Path::new("comic.cbz");
        let entries = mgr.list_images_from_buffer(&buf, path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_name, "page1.png");
        assert_eq!(entries[0].entry_index, 0);
        assert_eq!(
            mgr.read_entry_at(path, Some(&buf), entries[0].entry_index)
                .unwrap(),
            b"png"
        );
    }

    #[test]
    fn list_images_from_buffer_rejects_non_zip_extension() {
        let mgr = ArchiveManager::new(Arc::new(ExtensionRegistry::new()));
        let buf = crate::test_helpers::create_test_zip_buffer(&[("a.png", b"png")]);
        let err = mgr
            .list_images_from_buffer(&buf, Path::new("test.7z"))
            .err()
            .expect("7z のバッファ列挙は非対応");
        assert_eq!(err.to_string(), "オンデマンド列挙未対応");
    }

    #[test]
    fn supports_on_demand_returns_false_for_unsupported() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        assert!(!mgr.supports_on_demand(Path::new("test.xyz")));
    }

    #[test]
    fn supports_on_demand_for_zip() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        assert!(mgr.supports_on_demand(Path::new("test.zip")));
    }

    #[test]
    fn read_entry_fails_for_unsupported_extension() {
        let mgr = ArchiveManager::new(Arc::new(ExtensionRegistry::new()));
        let buf = crate::test_helpers::create_test_zip_buffer(&[("a.png", b"png")]);
        let err = mgr
            .read_entry_at(Path::new("test.xyz"), Some(&buf), 0)
            .unwrap_err();
        assert_eq!(err.to_string(), "未対応のアーカイブ形式: test.xyz");
    }

    #[test]
    fn extract_images_fails_for_unsupported_extension() {
        let mgr = ArchiveManager::new(Arc::new(ExtensionRegistry::new()));
        let dir = crate::test_helpers::TempDir::new("extract_unsupported");
        let path = dir.join("test.xyz");
        let output = dir.join("output");
        std::fs::create_dir(&output).unwrap();
        let sentinel = output.join("a.png");
        std::fs::write(&sentinel, b"existing").unwrap();
        crate::test_helpers::create_test_zip(&path, &[("a.png", b"png")]);
        let err = mgr.extract_images(&path, &output).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("未対応のアーカイブ形式: {}", path.display())
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
    }

    #[test]
    fn resolve_filename_no_extension() {
        let dir = crate::test_helpers::TempDir::new("resolve_noext");
        std::fs::write(dir.join("README"), b"documentation").unwrap();
        assert_eq!(resolve_filename(&dir, "README"), dir.join("README_2"));
        assert_eq!(std::fs::read(dir.join("README")).unwrap(), b"documentation");
    }

    #[test]
    fn resolve_filename_multiple_duplicates() {
        let dir = crate::test_helpers::TempDir::new("resolve_multi");
        std::fs::write(dir.join("img.png"), b"first").unwrap();
        std::fs::write(dir.join("img_2.png"), b"second").unwrap();
        assert_eq!(resolve_filename(&dir, "img.png"), dir.join("img_3.png"));
        assert_eq!(std::fs::read(dir.join("img.png")).unwrap(), b"first");
        assert_eq!(std::fs::read(dir.join("img_2.png")).unwrap(), b"second");
    }

    #[test]
    fn extract_filename_trailing_separator() {
        assert_eq!(extract_filename("folder/"), "");
        assert_eq!(extract_filename("folder\\"), "");
    }

    #[test]
    fn is_archive_rejects_path_without_filename() {
        let reg = Arc::new(ExtensionRegistry::new());
        let mgr = ArchiveManager::new(reg);
        assert!(!mgr.is_archive(Path::new("/")));
    }
}
