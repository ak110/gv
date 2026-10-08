use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result};

/// ファイルの論理的なソース情報
///
/// 通常ファイル・アーカイブ内エントリ・PDFページ・未展開コンテナを区別する
#[derive(Debug, Clone)]
pub enum FileSource {
    /// 通常のファイルシステム上のファイル
    File(PathBuf),
    /// アーカイブ内のエントリ
    ArchiveEntry {
        archive: PathBuf,
        entry: String,
        /// trueならオンデマンド取得、falseならtemp展開済み
        on_demand: bool,
        /// 一括展開した画像の実ファイル。オンデマンドではNone。
        temp_path: Option<PathBuf>,
        /// アーカイブ内エントリの再アクセス用インデックス。
        /// オンデマンド取得には`Some`が必須で、表示名から番号を推測しない。
        /// ブックマークの`None`はコンテナを開き直して番号付きの行へ再構築する。
        /// 一括展開済みの画像は番号を使わず、temp_pathから読み込む。
        entry_index: Option<u32>,
    },
    /// PDFのページ
    PdfPage { pdf_path: PathBuf, page_index: u32 },
    /// 未展開コンテナ (遅延読み込み用プレースホルダ)
    PendingContainer { container_path: PathBuf },
}

impl FileSource {
    /// 削除・移動できる通常ファイルのパス。
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            _ => None,
        }
    }

    /// 通常ファイルだけを削除・移動の対象にする。
    pub fn can_delete(&self) -> bool {
        self.file_path().is_some()
    }
    pub fn can_move(&self) -> bool {
        self.file_path().is_some()
    }

    /// 複製できる内容を持つか。未展開コンテナは対象外。
    pub fn can_copy(&self) -> bool {
        !self.is_pending_container()
    }

    /// デコーダへ渡す元の画像ファイル名。
    pub fn filename_hint(&self) -> String {
        match self {
            Self::ArchiveEntry { entry, .. } => crate::archive::extract_filename(entry).to_string(),
            Self::File(path) => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
            _ => self.default_save_name(),
        }
    }

    /// 画像内容を取得する。PDFはページを描画しPNGとして返す。
    pub fn read_bytes(
        &self,
        manager: &crate::archive::ArchiveManager,
        buffers: &std::sync::RwLock<std::collections::HashMap<PathBuf, crate::document::ZipBuffer>>,
    ) -> Result<Vec<u8>> {
        match self {
            Self::File(path) => {
                std::fs::read(path).with_context(|| format!("画像読み込み失敗: {}", path.display()))
            }
            Self::ArchiveEntry {
                archive,
                entry_index,
                temp_path,
                on_demand,
                ..
            } => {
                if !on_demand {
                    let path = temp_path
                        .as_ref()
                        .context("アーカイブ画像の展開先が未解決")?;
                    return Ok(std::fs::read(path)?);
                }
                let index = entry_index.context("オンデマンド取得用のエントリ番号がない")?;
                let read = |buffer: &[u8]| manager.read_entry_at(archive, Some(buffer), index);
                let cached = buffers.read().expect("zip_buffers lock poisoned");
                if let Some(buffer) = cached.get(archive) {
                    read(buffer.as_ref())
                } else {
                    drop(cached);
                    read(&std::fs::read(archive)?)
                }
            }
            Self::PdfPage {
                pdf_path,
                page_index,
            } => crate::image::encode_png(&crate::pdf_renderer::render_pdf_page_safe(
                pdf_path,
                *page_index,
            )?),
            Self::PendingContainer { .. } => anyhow::bail!("未展開コンテナからは取得できない"),
        }
    }

    /// 表示用パスを生成する。
    ///
    /// 戻り値は OS パスではなく UI 表示専用の論理パス文字列であり、
    /// `Path::join` で組み立てる対象ではない (`/` 区切り固定で Windows パスとも混在し得る)。
    /// ファイルシステム操作には `parent_dir()` / `default_save_name()` 等の専用メソッドを使うこと。
    pub fn display_path(&self) -> String {
        match self {
            FileSource::File(path) => path.display().to_string(),
            FileSource::ArchiveEntry { archive, entry, .. } => {
                format!("{}/{}", archive.display(), entry)
            }
            FileSource::PdfPage {
                pdf_path,
                page_index,
            } => {
                format!("{}/Page {}", pdf_path.display(), page_index + 1)
            }
            FileSource::PendingContainer { container_path } => {
                format!("{} (未展開)", container_path.display())
            }
        }
    }

    /// 未展開コンテナかどうか
    pub fn is_pending_container(&self) -> bool {
        matches!(self, FileSource::PendingContainer { .. })
    }

    /// ダイアログ初期ディレクトリ用: ソースの親ディレクトリを返す
    pub fn parent_dir(&self) -> Option<&Path> {
        match self {
            FileSource::File(path) => path.parent(),
            FileSource::ArchiveEntry { archive, .. } => archive.parent(),
            FileSource::PdfPage { pdf_path, .. } => pdf_path.parent(),
            FileSource::PendingContainer { container_path } => container_path.parent(),
        }
    }

    /// ダイアログ用デフォルトファイル名を返す
    pub fn default_save_name(&self) -> String {
        match self {
            FileSource::File(path) => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string(),
            FileSource::ArchiveEntry { archive, entry, .. } => {
                let archive_stem = archive
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("archive");
                let entry_filename = Path::new(entry)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image");
                format!("{archive_stem}_{entry_filename}")
            }
            FileSource::PdfPage {
                pdf_path,
                page_index,
            } => {
                let stem = pdf_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("pdf");
                format!("{stem}_page{}.png", page_index + 1)
            }
            FileSource::PendingContainer { container_path } => container_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("container")
                .to_string(),
        }
    }

    /// ダイアログ用デフォルトstem (拡張子なし) を返す (エクスポート用)
    pub fn default_save_stem(&self) -> String {
        let name = self.default_save_name();
        Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .to_string()
    }

    /// ブックマーク保存ダイアログの初期名に使う代表ステムを返す
    ///
    /// 通常ファイルは親ディレクトリ名、コンテナはコンテナファイル名のステム。
    /// 取得不能 (親なし・ステム抽出失敗・非UTF-8など) の場合は `None` を返し、
    /// 呼び出し側でフォールバックする。
    pub fn bookmark_default_stem(&self) -> Option<String> {
        let container_stem = |p: &Path| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(std::string::ToString::to_string)
        };
        match self {
            FileSource::File(path) => path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(std::string::ToString::to_string),
            FileSource::ArchiveEntry { archive, .. } => container_stem(archive),
            FileSource::PdfPage { pdf_path, .. } => container_stem(pdf_path),
            FileSource::PendingContainer { container_path } => container_stem(container_path),
        }
    }

    /// ブックマーク前回名キャッシュの一致判定キー用に絶対パス相当の識別子を返す
    ///
    /// 通常ファイルでは親ディレクトリのパスを返す。コンテナ系ではコンテナファイルのパスを返す。
    /// 同名別パスを区別するために代表ステム (`bookmark_default_stem`) とは分離している。
    /// 親ディレクトリを取得できない通常ファイルでは `None` を返す。
    pub fn bookmark_container_key(&self) -> Option<PathBuf> {
        match self {
            FileSource::File(path) => path.parent().map(Path::to_path_buf),
            FileSource::ArchiveEntry { archive, .. } => Some(archive.clone()),
            FileSource::PdfPage { pdf_path, .. } => Some(pdf_path.clone()),
            FileSource::PendingContainer { container_path } => Some(container_path.clone()),
        }
    }
}

impl fmt::Display for FileSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_path())
    }
}

/// 個々のファイル情報
pub struct FileInfo {
    pub source: FileSource, // 論理ソース (表示・保存・ブックマーク用)
    pub file_name: String,  // ソート用キャッシュ
    pub file_size: u64,
    pub modified: SystemTime,
    pub marked: bool,
    pub load_failed: bool, // デコード失敗フラグ (ナビゲーション時にスキップ)
}

impl FileInfo {
    /// パスからFileInfoを構築する (通常ファイル用)
    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("メタデータ取得失敗: {}", path.display()))?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(Self {
            source: FileSource::File(path.to_path_buf()),
            file_name,
            file_size: metadata.len(),
            modified,
            marked: false,
            load_failed: false,
        })
    }
}

#[cfg(test)]
impl FileSource {
    /// アーカイブパスを返す (アーカイブエントリ/PDFの場合)
    pub fn archive_path(&self) -> Option<&Path> {
        match self {
            FileSource::ArchiveEntry { archive, .. } => Some(archive),
            FileSource::PdfPage { pdf_path, .. } => Some(pdf_path),
            FileSource::PendingContainer { container_path } => Some(container_path),
            FileSource::File(_) => None,
        }
    }

    /// アーカイブエントリかどうか
    pub fn is_archive_entry(&self) -> bool {
        matches!(self, FileSource::ArchiveEntry { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn on_demand_requires_index_with_and_without_cached_zip() {
        let dir = crate::test_helpers::TempDir::new("on_demand_index");
        let archive = dir.join("images.zip");
        let png = crate::test_helpers::create_1x1_white_png();
        crate::test_helpers::create_test_zip(&archive, &[("image.png", &png)]);
        let manager = crate::test_helpers::test_archive_manager(&std::sync::Arc::new(
            crate::extension_registry::ExtensionRegistry::new(),
        ));
        let source = FileSource::ArchiveEntry {
            archive: archive.clone(),
            entry: "image.png".into(),
            on_demand: true,
            temp_path: None,
            entry_index: None,
        };
        for cached in [false, true] {
            let mut contents = std::collections::HashMap::new();
            if cached {
                contents.insert(
                    archive.clone(),
                    crate::document::ZipBuffer::Memory(std::fs::read(&archive).unwrap()),
                );
            }
            let buffers = std::sync::RwLock::new(contents);
            assert!(source.read_bytes(&manager, &buffers).is_err());
            for synchronous in [false, true] {
                assert!(
                    crate::image::decode_source(
                        &source,
                        &crate::test_helpers::test_decoder(),
                        &manager,
                        &buffers,
                        synchronous,
                    )
                    .is_err()
                );
            }

            // 表示名が一致しなくても、番号で選んだ内容を取得する。
            let mut indexed = source.clone();
            if let FileSource::ArchiveEntry {
                entry, entry_index, ..
            } = &mut indexed
            {
                *entry = "different-display-name.png".into();
                *entry_index = Some(0);
            }
            assert_eq!(indexed.read_bytes(&manager, &buffers).unwrap(), png);
            let mut invalid = indexed;
            if let FileSource::ArchiveEntry { entry_index, .. } = &mut invalid {
                *entry_index = Some(9);
            }
            assert!(invalid.read_bytes(&manager, &buffers).is_err());
        }

        // 一括展開済みの画像には番号を要求しない。
        let extracted = dir.join("extracted.png");
        std::fs::write(&extracted, &png).unwrap();
        let source = FileSource::ArchiveEntry {
            archive,
            entry: "image.png".into(),
            on_demand: false,
            temp_path: Some(extracted),
            entry_index: None,
        };
        assert_eq!(
            source
                .read_bytes(
                    &manager,
                    &std::sync::RwLock::new(std::collections::HashMap::new())
                )
                .unwrap(),
            png
        );
    }

    #[test]
    fn bookmark_zip_rebuilds_index_before_reading() {
        let dir = crate::test_helpers::TempDir::new("bookmark_zip_index");
        let archive = dir.join("images.zip");
        let first = crate::test_helpers::solid_image(2, 1, [10, 20, 30, 255]);
        let selected = crate::test_helpers::solid_image(3, 2, [70, 80, 90, 255]);
        let first_png = crate::image::encode_png(&first).unwrap();
        let selected_png = crate::image::encode_png(&selected).unwrap();
        crate::test_helpers::create_test_zip(
            &archive,
            &[("first.png", &first_png), ("selected.png", &selected_png)],
        );
        let bookmark = dir.join("images.gvbm");
        std::fs::write(
            &bookmark,
            format!(
                "# gv3 bookmark v1\n# index: 0\narchive\t{}\tselected.png\n",
                archive.display()
            ),
        )
        .unwrap();
        let data = crate::bookmark::load_bookmark_from_path(&bookmark, &|_| true).unwrap();
        assert!(matches!(
            &data.entries[0],
            FileSource::ArchiveEntry {
                entry_index: None,
                ..
            }
        ));
        let (mut document, _events) = crate::test_helpers::test_document();
        document.load_bookmark_data(data).unwrap();
        assert!(matches!(
            document.current_source(),
            Some(FileSource::ArchiveEntry {
                on_demand: true,
                entry_index: Some(1),
                ..
            })
        ));
        assert_eq!(document.read_file_data_current().unwrap(), selected_png);
        crate::test_helpers::assert_same_image(document.current_image().unwrap(), &selected);
    }

    #[test]
    fn from_path_valid_file() {
        let dir = crate::test_helpers::TempDir::new("file_info");
        let file_path = dir.join("test.png");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"dummy content").unwrap();
        drop(f);

        let info = FileInfo::from_path(&file_path).unwrap();
        assert_eq!(info.file_name, "test.png");
        assert_eq!(info.file_size, 13);
        assert!(!info.marked);
        assert!(!info.load_failed);
        assert!(!info.source.is_archive_entry());
    }

    #[test]
    fn file_source_display_path() {
        let source = FileSource::File(PathBuf::from(r"C:\images\test.jpg"));
        assert_eq!(source.display_path(), r"C:\images\test.jpg");
        assert!(!source.is_archive_entry());
        assert!(source.archive_path().is_none());

        let source = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\archive.zip"),
            entry: "folder/image.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: Some(0),
        };
        assert_eq!(source.display_path(), r"C:\archive.zip/folder/image.png");
        assert!(source.is_archive_entry());
        assert_eq!(source.archive_path().unwrap(), Path::new(r"C:\archive.zip"));
    }

    #[test]
    fn pdf_page_source() {
        let source = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\docs\test.pdf"),
            page_index: 2,
        };
        assert_eq!(source.display_path(), r"C:\docs\test.pdf/Page 3");
        assert!(!source.is_archive_entry());
        assert!(!source.can_delete());
        assert_eq!(
            source.archive_path().unwrap(),
            Path::new(r"C:\docs\test.pdf")
        );

        // 通常ファイルは削除対象になる
        let file_source = FileSource::File(PathBuf::from(r"C:\images\test.jpg"));
        assert!(file_source.can_delete());

        // アーカイブ内のエントリは削除対象にならない
        let archive_source = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\archive.zip"),
            entry: "img.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
        assert!(!archive_source.can_delete());
    }

    #[test]
    fn from_path_nonexistent() {
        let result = FileInfo::from_path(Path::new("nonexistent_file_xyz.png"));
        assert!(result.is_err());
    }

    #[test]
    fn parent_dir_for_each_source() {
        let file = FileSource::File(PathBuf::from(r"C:\images\test.jpg"));
        assert_eq!(file.parent_dir().unwrap(), Path::new(r"C:\images"));

        let archive = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\archives\photos.zip"),
            entry: "folder/sunset.png".to_string(),
            on_demand: true,
            temp_path: None,
            entry_index: Some(0),
        };
        assert_eq!(archive.parent_dir().unwrap(), Path::new(r"C:\archives"));

        let pdf = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\docs\report.pdf"),
            page_index: 0,
        };
        assert_eq!(pdf.parent_dir().unwrap(), Path::new(r"C:\docs"));
    }

    #[test]
    fn default_save_name_for_each_source() {
        let file = FileSource::File(PathBuf::from(r"C:\images\sunset.jpg"));
        assert_eq!(file.default_save_name(), "sunset.jpg");

        let archive = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\photos.zip"),
            entry: "folder/sunset.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
        assert_eq!(archive.default_save_name(), "photos_sunset.png");

        let pdf = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\docs\report.pdf"),
            page_index: 2,
        };
        assert_eq!(pdf.default_save_name(), "report_page3.png");
    }

    #[test]
    fn default_save_stem_strips_extension() {
        let file = FileSource::File(PathBuf::from(r"C:\images\sunset.jpg"));
        assert_eq!(file.default_save_stem(), "sunset");

        let archive = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\photos.zip"),
            entry: "img.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
        assert_eq!(archive.default_save_stem(), "photos_img");

        let pdf = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\doc.pdf"),
            page_index: 0,
        };
        assert_eq!(pdf.default_save_stem(), "doc_page1");
    }

    #[test]
    fn bookmark_default_stem_for_each_source() {
        // 通常ファイル (親フォルダあり): 親フォルダ名を返す
        let file = FileSource::File(PathBuf::from(r"C:\photos\sunset.jpg"));
        assert_eq!(file.bookmark_default_stem().as_deref(), Some("photos"));

        // 通常ファイル (ルート直下): parent() がドライブルート `C:\` を返し、
        // その file_name() が `None` を返すため、最終結果は `None`
        let root_file = FileSource::File(PathBuf::from(r"C:\sunset.jpg"));
        assert_eq!(root_file.bookmark_default_stem(), None);

        // アーカイブ: アーカイブファイル名のステム
        let archive = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\archive.zip"),
            entry: "folder/image.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
        assert_eq!(archive.bookmark_default_stem().as_deref(), Some("archive"));

        // PDF: PDFファイル名のステム
        let pdf = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\docs\report.pdf"),
            page_index: 0,
        };
        assert_eq!(pdf.bookmark_default_stem().as_deref(), Some("report"));

        // 未展開コンテナ: コンテナファイル名のステム
        let pending = FileSource::PendingContainer {
            container_path: PathBuf::from(r"C:\unopened.zip"),
        };
        assert_eq!(pending.bookmark_default_stem().as_deref(), Some("unopened"));
    }

    #[test]
    fn bookmark_container_key_for_each_source() {
        // 通常ファイル (親ディレクトリあり): 親ディレクトリのパスを返す
        let file = FileSource::File(PathBuf::from(r"C:\photos\sunset.jpg"));
        assert_eq!(
            file.bookmark_container_key().as_deref(),
            Some(Path::new(r"C:\photos"))
        );

        // 通常ファイル (ルート直下): parent() がドライブルート `C:\` を返す。
        // 代表ステムでは `None` になるケースでも、コンテナ識別キーは判定に利用できる
        let root_file = FileSource::File(PathBuf::from(r"C:\sunset.jpg"));
        assert_eq!(
            root_file.bookmark_container_key().as_deref(),
            Some(Path::new(r"C:\"))
        );

        // アーカイブ: アーカイブファイルのパス
        let archive = FileSource::ArchiveEntry {
            archive: PathBuf::from(r"C:\archive.zip"),
            entry: "folder/image.png".to_string(),
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
        assert_eq!(
            archive.bookmark_container_key().as_deref(),
            Some(Path::new(r"C:\archive.zip"))
        );

        // PDF: PDFファイルのパス
        let pdf = FileSource::PdfPage {
            pdf_path: PathBuf::from(r"C:\docs\report.pdf"),
            page_index: 0,
        };
        assert_eq!(
            pdf.bookmark_container_key().as_deref(),
            Some(Path::new(r"C:\docs\report.pdf"))
        );

        // 未展開コンテナ: コンテナファイルのパス
        let pending = FileSource::PendingContainer {
            container_path: PathBuf::from(r"C:\unopened.zip"),
        };
        assert_eq!(
            pending.bookmark_container_key().as_deref(),
            Some(Path::new(r"C:\unopened.zip"))
        );

        // 同名の別パスは別キーとして区別される (代表ステムでは区別できない欠陥への対処)
        let a = FileSource::File(PathBuf::from(r"C:\a\photos\x.jpg"));
        let b = FileSource::File(PathBuf::from(r"D:\b\photos\y.jpg"));
        assert_ne!(a.bookmark_container_key(), b.bookmark_container_key());
    }
    #[test]
    fn copy_content_and_capabilities_for_mixed_sources() {
        let dir = crate::test_helpers::TempDir::new("source_copy");
        let image_path = dir.join("original.png");
        let temp_path = dir.join("unrelated.tmp");
        let png = crate::test_helpers::create_1x1_white_png();
        std::fs::write(&image_path, &png).unwrap();
        std::fs::write(&temp_path, &png).unwrap();
        let archive = dir.join("archive.zip");
        let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file(
                "folder/original.png",
                ::zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(&png).unwrap();
        writer.finish().unwrap();
        let pdf_path = dir.join("document.pdf");
        std::fs::write(&pdf_path, crate::test_helpers::minimal_pdf()).unwrap();
        let manager = crate::test_helpers::test_archive_manager(&std::sync::Arc::new(
            crate::extension_registry::ExtensionRegistry::new(),
        ));
        let buffers = std::sync::RwLock::new(std::collections::HashMap::new());
        let sources = [
            FileSource::File(image_path),
            FileSource::ArchiveEntry {
                archive: archive.clone(),
                entry: "folder/original.png".into(),
                on_demand: false,
                temp_path: Some(temp_path),
                entry_index: None,
            },
            FileSource::ArchiveEntry {
                archive: archive.clone(),
                entry: "folder/original.png".into(),
                on_demand: true,
                temp_path: None,
                entry_index: Some(0),
            },
        ];
        for (index, source) in sources.iter().enumerate() {
            assert!(source.can_copy());
            assert_eq!(source.can_delete(), index == 0);
            assert_eq!(source.can_move(), index == 0);
            assert_eq!(source.filename_hint(), "original.png");
            assert_eq!(source.read_bytes(&manager, &buffers).unwrap(), png);
        }
        let page = FileSource::PdfPage {
            pdf_path,
            page_index: 0,
        };
        let bytes = page.read_bytes(&manager, &buffers).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let rendered = image::load_from_memory(&bytes).unwrap();
        assert_eq!((rendered.width(), rendered.height()), (6, 3));
        assert_eq!(page.default_save_name(), "document_page1.png");
        assert!(!page.can_move());
        let pending = FileSource::PendingContainer {
            container_path: archive,
        };
        assert!(!pending.can_copy());
        assert!(pending.read_bytes(&manager, &buffers).is_err());
    }
}
