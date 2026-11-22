mod legacy;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;

use crate::file_info::FileSource;

pub const BOOKMARK_EXTENSION: &str = ".gvbm";
pub const OLD_BOOKMARK_EXTENSION: &str = ".gv3bm";
pub const LEGACY_BOOKMARK_EXTENSION: &str = ".gvb";
pub const BOOKMARK_EXTENSIONS: &[&str] = &[
    BOOKMARK_EXTENSION,
    OLD_BOOKMARK_EXTENSION,
    LEGACY_BOOKMARK_EXTENSION,
];

pub struct BookmarkData {
    pub entries: Vec<FileSource>,
    pub index: usize,
}

pub fn is_bookmark_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        BOOKMARK_EXTENSIONS
            .iter()
            .any(|extension| e.eq_ignore_ascii_case(extension.trim_start_matches('.')))
    })
}

pub fn bookmark_dir() -> PathBuf {
    crate::paths::bookmark_dir()
}

pub fn build_initial_save_name(
    previous_name: Option<&str>,
    first_source: Option<&FileSource>,
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs();
    build_initial_save_name_at(previous_name, first_source, now)
}

fn build_initial_save_name_at(
    previous_name: Option<&str>,
    first_source: Option<&FileSource>,
    now_unix_secs: u64,
) -> String {
    if let Some(name) = previous_name {
        return name.to_string();
    }
    first_source
        .and_then(FileSource::bookmark_default_stem)
        .filter(|s| !s.is_empty())
        .map_or_else(
            || format!("bookmark_{now_unix_secs}{BOOKMARK_EXTENSION}"),
            |stem| format!("{stem}{BOOKMARK_EXTENSION}"),
        )
}

pub fn save_bookmark(
    hwnd: HWND,
    entries: &[FileSource],
    current_index: Option<usize>,
    initial_name: &str,
) -> Result<Option<PathBuf>> {
    let dir = bookmark_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "警告: ブックマークディレクトリ作成失敗: {} ({e})",
            dir.display()
        );
    }

    let save_path = crate::ui::file_dialog::save_file_dialog(
        hwnd,
        crate::ui::file_dialog::SaveFileDialogParams {
            default_name: initial_name,
            filter_name: "ぐらびゅブックマーク",
            filter_ext: &format!("*{BOOKMARK_EXTENSION}"),
            default_ext: BOOKMARK_EXTENSION.trim_start_matches('.'),
            initial_dir: Some(&dir),
            ..Default::default()
        },
    )?;

    let Some(save_path) = save_path else {
        return Ok(None);
    };

    let content = format_bookmark(entries, current_index);
    crate::shell::file_operations::write_atomic(&save_path, content.as_bytes())
        .with_context(|| format!("ブックマーク保存失敗: {}", save_path.display()))?;

    Ok(Some(save_path))
}

fn format_bookmark(entries: &[FileSource], current_index: Option<usize>) -> String {
    let mut content = String::new();
    content.push_str("# gv3 bookmark v1\n");
    let _ = writeln!(content, "# index: {}", current_index.unwrap_or(0));

    for source in entries {
        match source {
            FileSource::File(path) => {
                let _ = writeln!(content, "file\t{}", path.display());
            }
            FileSource::ArchiveEntry { archive, entry, .. } => {
                let _ = writeln!(content, "archive\t{}\t{}", archive.display(), entry);
            }
            FileSource::PdfPage {
                pdf_path,
                page_index,
            } => {
                let _ = writeln!(content, "pdf\t{}\t{}", pdf_path.display(), page_index);
            }
            FileSource::PendingContainer { .. } => {}
        }
    }
    content
}

pub fn load_bookmark_from_path(
    path: &Path,
    is_archive: &impl Fn(&Path) -> bool,
) -> Result<BookmarkData> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("ブックマーク読み込み失敗: {}", path.display()))?;

    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(legacy::parse_legacy_bookmark_utf16le(rest, is_archive));
    }

    let content = std::str::from_utf8(&bytes)
        .with_context(|| format!("ブックマークのUTF-8デコード失敗: {}", path.display()))?;
    Ok(parse_bookmark(content))
}

fn parse_bookmark(content: &str) -> BookmarkData {
    let mut entries = Vec::new();
    let mut index = 0;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            if let Some(idx_str) = line.strip_prefix("# index:")
                && let Ok(idx) = idx_str.trim().parse::<usize>()
            {
                index = idx;
            }
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        match parts.as_slice() {
            ["file", path] => {
                entries.push(FileSource::File(PathBuf::from(path)));
            }
            ["archive", archive_path, entry] => {
                entries.push(FileSource::ArchiveEntry {
                    archive: PathBuf::from(archive_path),
                    entry: entry.to_string(),
                    on_demand: false,
                    temp_path: None,
                    entry_index: None,
                });
            }
            ["pdf", pdf_path, page_index_str] => {
                if let Ok(page_index) = page_index_str.parse::<u32>() {
                    entries.push(FileSource::PdfPage {
                        pdf_path: PathBuf::from(pdf_path),
                        page_index,
                    });
                }
            }
            _ => {
                entries.push(FileSource::File(PathBuf::from(line)));
            }
        }
    }

    if !entries.is_empty() && index >= entries.len() {
        index = entries.len() - 1;
    }

    BookmarkData { entries, index }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn build_initial_save_name_uses_previous_name_when_present() {
        let source = FileSource::File(PathBuf::from(r"C:\photos\sunset.jpg"));
        let name = build_initial_save_name(Some("custom.gvbm"), Some(&source));
        assert_eq!(name, "custom.gvbm");
    }

    #[test]
    fn build_initial_save_name_uses_stem_when_no_previous_name() {
        let source = FileSource::File(PathBuf::from(r"C:\photos\sunset.jpg"));
        let name = build_initial_save_name(None, Some(&source));
        assert_eq!(name, "photos.gvbm");
    }

    #[test]
    fn build_initial_save_name_falls_back_to_timestamp_when_no_source() {
        assert_eq!(
            build_initial_save_name_at(None, None, 1_700_000_000),
            "bookmark_1700000000.gvbm"
        );
    }

    #[test]
    fn format_bookmark_round_trips_through_parse_bookmark() {
        let entries = [
            FileSource::File(PathBuf::from(r"C:\images\a.jpg")),
            FileSource::ArchiveEntry {
                archive: PathBuf::from(r"C:\archive.zip"),
                entry: "folder/b.png".to_string(),
                on_demand: true,
                temp_path: None,
                entry_index: Some(3),
            },
            FileSource::PendingContainer {
                container_path: PathBuf::from(r"C:\pending.zip"),
            },
            FileSource::PdfPage {
                pdf_path: PathBuf::from(r"C:\docs\c.pdf"),
                page_index: 2,
            },
        ];
        let content = format_bookmark(&entries, Some(2));
        assert_eq!(
            content,
            "# gv3 bookmark v1\n# index: 2\nfile\tC:\\images\\a.jpg\n\
             archive\tC:\\archive.zip\tfolder/b.png\npdf\tC:\\docs\\c.pdf\t2\n"
        );
        let data = parse_bookmark(&content);
        assert_eq!(data.index, 2);
        assert_eq!(format_bookmark(&data.entries, Some(data.index)), content);
    }

    #[test]
    fn parse_bookmark_normal() {
        let content = r"# gv3 bookmark v1
# index: 1
file	C:\images\test.jpg
file	C:\images\test2.png
";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 1);
        assert!(
            matches!(&data.entries[0], FileSource::File(p) if p == Path::new(r"C:\images\test.jpg"))
        );
    }

    #[test]
    fn parse_bookmark_with_archive() {
        let content = r"# gv3 bookmark v1
# index: 0
file	C:\images\test.jpg
archive	C:\archive.zip	folder/image.png
";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 2);
        assert!(matches!(
            &data.entries[1],
            FileSource::ArchiveEntry { archive, entry, .. }
            if archive == Path::new(r"C:\archive.zip") && entry == "folder/image.png"
        ));
    }

    #[test]
    fn parse_bookmark_with_pdf() {
        let content = r"# gv3 bookmark v1
# index: 2
pdf	C:\docs\test.pdf	0
pdf	C:\docs\test.pdf	1
pdf	C:\docs\test.pdf	2
";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 3);
        assert_eq!(data.index, 2);
        assert!(matches!(
            &data.entries[1],
            FileSource::PdfPage { pdf_path, page_index }
            if pdf_path == Path::new(r"C:\docs\test.pdf") && *page_index == 1
        ));
    }

    #[test]
    fn parse_bookmark_index_clamp() {
        let content = "# index: 999\nfile\ttest.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_bookmark_empty_content() {
        let data = parse_bookmark("");
        assert!(data.entries.is_empty());
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_bookmark_header_only_no_entries() {
        let content = "# gv3 bookmark v1\n# index: 5\n";
        let data = parse_bookmark(content);
        assert!(data.entries.is_empty());
        assert_eq!(data.index, 5);
    }

    #[test]
    fn parse_bookmark_no_index_comment() {
        let content = "# gv3 bookmark v1\nfile\ttest.jpg\nfile\ttest2.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_bookmark_invalid_index_value() {
        let content = "# index: abc\nfile\ttest.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.index, 0);
        assert_eq!(data.entries.len(), 1);
    }

    #[test]
    fn parse_bookmark_negative_index() {
        let content = "# index: -1\nfile\ttest.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_bookmark_blank_lines_ignored() {
        let content = "# gv3 bookmark v1\n\n\nfile\ttest.jpg\n\nfile\ttest2.jpg\n\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 2);
    }

    #[test]
    fn parse_bookmark_whitespace_lines_ignored() {
        let content = "   \n\tfile\ta.jpg\n   \n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 1);
        assert!(matches!(&data.entries[0], FileSource::File(p) if p == Path::new("a.jpg")));
    }

    #[test]
    fn parse_bookmark_unknown_type_falls_back_to_file() {
        let content = "unknown\tpath\textra\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 1);
        assert!(matches!(&data.entries[0], FileSource::File(_)));
    }

    #[test]
    fn parse_bookmark_bare_path_without_type() {
        let content = r"C:\images\photo.jpg";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 1);
        assert!(
            matches!(&data.entries[0], FileSource::File(p) if p == Path::new(r"C:\images\photo.jpg"))
        );
    }

    #[test]
    fn parse_bookmark_pdf_invalid_page_index() {
        let content = "pdf\tC:\\test.pdf\tabc\n";
        let data = parse_bookmark(content);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn parse_bookmark_pdf_negative_page_index() {
        let content = "pdf\tC:\\test.pdf\t-1\n";
        let data = parse_bookmark(content);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn parse_bookmark_archive_on_demand_is_false() {
        let content = "archive\tC:\\test.zip\timg.png\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 1);
        assert!(matches!(
            &data.entries[0],
            FileSource::ArchiveEntry { on_demand, .. } if !on_demand
        ));
    }

    #[test]
    fn parse_bookmark_mixed_entry_types() {
        let content = r"# gv3 bookmark v1
# index: 2
file	C:\images\a.jpg
archive	C:\archive.zip	inner/b.png
pdf	C:\docs\doc.pdf	3
file	C:\images\c.bmp
";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 4);
        assert_eq!(data.index, 2);
        assert!(matches!(&data.entries[0], FileSource::File(_)));
        assert!(matches!(&data.entries[1], FileSource::ArchiveEntry { .. }));
        assert!(matches!(
            &data.entries[2],
            FileSource::PdfPage { page_index, .. } if *page_index == 3
        ));
        assert!(matches!(&data.entries[3], FileSource::File(_)));
    }

    #[test]
    fn parse_bookmark_index_clamp_exact_boundary() {
        let content = "# index: 3\nfile\ta.jpg\nfile\tb.jpg\nfile\tc.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 3);
        assert_eq!(data.index, 2);
    }

    #[test]
    fn parse_bookmark_index_within_range() {
        let content = "# index: 2\nfile\ta.jpg\nfile\tb.jpg\nfile\tc.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.index, 2);
    }

    #[test]
    fn parse_bookmark_comment_lines_ignored() {
        let content = "# gv3 bookmark v1\n# some comment\n# another\nfile\ta.jpg\n";
        let data = parse_bookmark(content);
        assert_eq!(data.entries.len(), 1);
    }
}
