use std::path::{Path, PathBuf};

use crate::bookmark::BookmarkData;
use crate::file_info::FileSource;

pub(super) fn parse_legacy_bookmark_utf16le(
    utf16le_bytes: &[u8],
    is_archive: &impl Fn(&Path) -> bool,
) -> BookmarkData {
    let text = decode_utf16le_lossy(utf16le_bytes);
    parse_legacy_bookmark(&text, is_archive)
}

fn decode_utf16le_lossy(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

struct ParsedSections {
    current: Option<String>,
    files: Vec<String>,
    archives: Vec<String>,
}

fn parse_sections(text: &str) -> ParsedSections {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        None,
        Current,
        Files,
        Archives,
    }

    let mut section = Section::None;
    let mut current_path: Option<String> = None;
    let mut raw_files: Vec<String> = Vec::new();
    let mut raw_archives: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = Section::None;
            continue;
        }

        if trimmed == "current:" {
            section = Section::Current;
            continue;
        }
        if trimmed == "files:" {
            section = Section::Files;
            continue;
        }
        if trimmed == "archives:" {
            section = Section::Archives;
            continue;
        }

        match section {
            Section::Current => {
                if current_path.is_none() {
                    current_path = Some(trimmed.to_string());
                }
            }
            Section::Files | Section::None => {
                raw_files.push(trimmed.to_string());
            }
            Section::Archives => {
                raw_archives.push(trimmed.to_string());
            }
        }
    }

    ParsedSections {
        current: current_path,
        files: raw_files,
        archives: raw_archives,
    }
}

fn parse_legacy_bookmark(text: &str, is_archive: &impl Fn(&Path) -> bool) -> BookmarkData {
    let sections = parse_sections(text);

    let index = sections
        .current
        .as_deref()
        .and_then(|cur| {
            sections
                .files
                .iter()
                .position(|f| f.eq_ignore_ascii_case(cur))
        })
        .unwrap_or(0);

    let mut entries: Vec<FileSource> = sections
        .files
        .iter()
        .map(|raw| classify_raw_path(raw, is_archive))
        .collect();

    if !sections.archives.is_empty() {
        let known_archives: Vec<PathBuf> = entries
            .iter()
            .filter_map(|e| match e {
                FileSource::ArchiveEntry { archive, .. } => Some(archive.clone()),
                FileSource::PendingContainer { container_path } => Some(container_path.clone()),
                _ => None,
            })
            .collect();

        for raw_archive in &sections.archives {
            let archive_path = PathBuf::from(raw_archive.as_str());
            if !known_archives
                .iter()
                .any(|k| k.as_os_str().eq_ignore_ascii_case(archive_path.as_os_str()))
            {
                entries.push(FileSource::PendingContainer {
                    container_path: archive_path,
                });
            }
        }
    }

    let index = if entries.is_empty() {
        0
    } else {
        index.min(entries.len() - 1)
    };

    BookmarkData { entries, index }
}

fn classify_raw_path(raw: &str, is_archive: &impl Fn(&Path) -> bool) -> FileSource {
    if let Some((archive, entry)) = split_archive_path(raw, is_archive) {
        return FileSource::ArchiveEntry {
            archive,
            entry,
            on_demand: false,
            temp_path: None,
            entry_index: None,
        };
    }

    let full = Path::new(raw);
    if is_archive(full) {
        return FileSource::PendingContainer {
            container_path: full.to_path_buf(),
        };
    }

    FileSource::File(PathBuf::from(raw))
}

fn split_archive_path(raw: &str, is_archive: &impl Fn(&Path) -> bool) -> Option<(PathBuf, String)> {
    let full = Path::new(raw);

    let mut matched: Option<&Path> = None;
    for ancestor in full.ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            break;
        }
        if is_archive(ancestor) {
            matched = Some(ancestor);
        }
    }

    let archive = matched?;
    let rel = full.strip_prefix(archive).ok()?;
    let entry = rel.to_string_lossy().replace('\\', "/");

    if entry.is_empty() {
        None
    } else {
        Some((archive.to_path_buf(), entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_is_archive;
    use crate::test_helpers::to_utf16le;

    #[test]
    fn parse_normal() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nC:\\images\\b.jpg\r\nfiles:\r\nC:\\images\\a.jpg\r\nC:\\images\\b.jpg\r\nC:\\images\\c.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 3);
        assert_eq!(data.index, 1);
        assert!(
            matches!(&data.entries[0], FileSource::File(p) if p == Path::new(r"C:\images\a.jpg"))
        );
        assert!(
            matches!(&data.entries[1], FileSource::File(p) if p == Path::new(r"C:\images\b.jpg"))
        );
        assert!(
            matches!(&data.entries[2], FileSource::File(p) if p == Path::new(r"C:\images\c.jpg"))
        );
    }

    #[test]
    fn parse_with_archive_entries() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nE:\\book.zip\\01.jpg\r\nfiles:\r\nE:\\book.zip\\01.jpg\r\nE:\\book.zip\\02.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 0);
        assert!(matches!(
            &data.entries[0],
            FileSource::ArchiveEntry { archive, entry, on_demand: false, .. }
            if archive == Path::new(r"E:\book.zip") && entry == "01.jpg"
        ));
        assert!(matches!(
            &data.entries[1],
            FileSource::ArchiveEntry { archive, entry, on_demand: false, .. }
            if archive == Path::new(r"E:\book.zip") && entry == "02.jpg"
        ));
    }

    #[test]
    fn parse_standalone_archive_becomes_pending_container() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nE:\\vol01.zip\r\nfiles:\r\nE:\\vol01.zip\r\nE:\\vol02.zip\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert!(matches!(
            &data.entries[0],
            FileSource::PendingContainer { container_path } if container_path == Path::new(r"E:\vol01.zip")
        ));
        assert!(matches!(
            &data.entries[1],
            FileSource::PendingContainer { container_path } if container_path == Path::new(r"E:\vol02.zip")
        ));
    }

    #[test]
    fn parse_mixed_archive_and_entries() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nvol2.zip\\01.jpg\r\nfiles:\r\nvol1.zip\r\nvol2.zip\\01.jpg\r\nvol2.zip\\02.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 3);
        assert_eq!(data.index, 1);
        assert!(matches!(
            &data.entries[0],
            FileSource::PendingContainer { container_path } if container_path == Path::new("vol1.zip")
        ));
        for (source, expected) in data.entries[1..].iter().zip(["01.jpg", "02.jpg"]) {
            assert!(
                matches!(source, FileSource::ArchiveEntry { archive, entry, .. }
                if archive == Path::new("vol2.zip") && entry == expected)
            );
        }
    }

    #[test]
    fn parse_unc_path() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\n\\\\server\\share\\dir\\a.jpg\r\nfiles:\r\n\\\\server\\share\\dir\\a.jpg\r\n\\\\server\\share\\dir\\b.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 0);
        assert!(matches!(
            &data.entries[0],
            FileSource::File(p) if p == Path::new(r"\\server\share\dir\a.jpg")
        ));
    }

    #[test]
    fn parse_nested_archive_ext_picks_outermost() {
        let text = "files:\r\nC:\\a.zip\\b.zip\\c.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 1);
        assert!(matches!(
            &data.entries[0],
            FileSource::ArchiveEntry { archive, entry, .. }
            if archive == Path::new(r"C:\a.zip") && entry == "b.zip/c.jpg"
        ));
    }

    #[test]
    fn parse_missing_header() {
        let text = "current:\r\ntest2.jpg\r\nfiles:\r\ntest.jpg\r\ntest2.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 1);
        for (source, expected) in data.entries.iter().zip(["test.jpg", "test2.jpg"]) {
            assert!(matches!(source, FileSource::File(path) if path == Path::new(expected)));
        }
    }

    #[test]
    fn parse_no_current_section() {
        let text = "[gvbookmark/ver:1]\r\nfiles:\r\na.jpg\r\nb.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_current_not_in_files() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nnotexist.jpg\r\nfiles:\r\na.jpg\r\nb.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_index_matching_case_insensitive() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nc:\\Foo.jpg\r\nfiles:\r\nC:\\bar.jpg\r\nC:\\foo.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.index, 1);
    }

    #[test]
    fn parse_index_points_to_non_first_archive() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nE:\\vol2.zip\\003.jpg\r\nfiles:\r\nE:\\vol1.zip\\001.jpg\r\nE:\\vol1.zip\\002.jpg\r\nE:\\vol2.zip\\003.jpg\r\nE:\\vol2.zip\\004.jpg\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 4);
        assert_eq!(data.index, 2);
    }

    #[test]
    fn parse_empty_after_bom() {
        let dir = crate::test_helpers::TempDir::new("legacy_bom");
        let path = dir.join("bookmark.txt");
        std::fs::write(&path, [0xFF, 0xFE]).unwrap();
        let data = crate::bookmark::load_bookmark_from_path(&path, &test_is_archive).unwrap();
        assert!(data.entries.is_empty());
        assert_eq!(data.index, 0);
    }

    #[test]
    fn parse_odd_byte_count() {
        let mut bytes = to_utf16le("files:\r\na.jpg");
        bytes.push(0xFF);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 1);
        for (source, expected) in data.entries.iter().zip(["a.jpg"]) {
            assert!(matches!(source, FileSource::File(path) if path == Path::new(expected)));
        }
    }

    #[test]
    fn parse_ignores_blank_lines() {
        let text = "[gvbookmark/ver:1]\r\n\r\nfiles:\r\n\r\na.jpg\r\n\r\nb.jpg\r\n\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        for (source, expected) in data.entries.iter().zip(["a.jpg", "b.jpg"]) {
            assert!(matches!(source, FileSource::File(path) if path == Path::new(expected)));
        }
    }

    #[test]
    fn split_at_zip() {
        let result = split_archive_path(r"C:\data\test.zip\img\001.jpg", &test_is_archive);
        let (archive, entry) = result.unwrap();
        assert_eq!(archive, Path::new(r"C:\data\test.zip"));
        assert_eq!(entry, "img/001.jpg");
    }

    #[test]
    fn split_uppercase_cbr() {
        let result = split_archive_path(r"D:\comics\vol.CBR\page1.png", &test_is_archive);
        let (archive, entry) = result.unwrap();
        assert_eq!(archive, Path::new(r"D:\comics\vol.CBR"));
        assert_eq!(entry, "page1.png");
    }

    #[test]
    fn split_no_archive_returns_none() {
        let result = split_archive_path(r"C:\images\photo.jpg", &test_is_archive);
        assert!(result.is_none());
    }

    #[test]
    fn split_archive_itself_returns_none() {
        let result = split_archive_path(r"C:\data\test.zip", &test_is_archive);
        assert!(result.is_none());
    }

    #[test]
    fn parse_archives_section_adds_unknown_containers() {
        let text = "[gvbookmark/ver:1]\r\ncurrent:\r\nE:\\vol1.zip\\01.jpg\r\nfiles:\r\nE:\\vol1.zip\\01.jpg\r\nE:\\vol1.zip\\02.jpg\r\narchives:\r\nE:\\vol1.zip\r\nE:\\vol2.zip\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 3);
        assert!(matches!(&data.entries[0], FileSource::ArchiveEntry { .. }));
        assert!(matches!(&data.entries[1], FileSource::ArchiveEntry { .. }));
        assert!(matches!(
            &data.entries[2],
            FileSource::PendingContainer { container_path }
            if container_path == Path::new(r"E:\vol2.zip")
        ));
    }

    #[test]
    fn parse_archives_section_no_duplicates() {
        let text =
            "[gvbookmark/ver:1]\r\nfiles:\r\nE:\\vol1.zip\\01.jpg\r\narchives:\r\nE:\\vol1.zip\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 1);
        assert!(matches!(&data.entries[0], FileSource::ArchiveEntry { .. }));
    }

    #[test]
    fn parse_archives_only_no_files() {
        let text = "[gvbookmark/ver:1]\r\narchives:\r\nE:\\vol1.zip\r\nE:\\vol2.zip\r\n";
        let bytes = to_utf16le(text);
        let data = parse_legacy_bookmark_utf16le(&bytes, &test_is_archive);
        assert_eq!(data.entries.len(), 2);
        assert!(matches!(
            &data.entries[0],
            FileSource::PendingContainer { container_path } if container_path == Path::new(r"E:\vol1.zip")
        ));
        assert!(matches!(
            &data.entries[1],
            FileSource::PendingContainer { container_path } if container_path == Path::new(r"E:\vol2.zip")
        ));
    }
}
