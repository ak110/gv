use std::collections::HashMap;
use std::path::PathBuf;

use crate::file_info::{FileInfo, FileSource};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum GroupKey {
    Folder(PathBuf),
    Archive(PathBuf),
}

pub(crate) fn group_key(info: &FileInfo) -> GroupKey {
    match &info.source {
        FileSource::ArchiveEntry { archive, .. } => GroupKey::Archive(archive.clone()),
        FileSource::PdfPage { pdf_path, .. } => GroupKey::Archive(pdf_path.clone()),
        FileSource::PendingContainer { container_path } => {
            GroupKey::Archive(container_path.clone())
        }
        FileSource::File(path) => GroupKey::Folder(
            path.parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default(),
        ),
    }
}

pub(crate) fn compute_group_layout(files: &[FileInfo]) -> (Vec<usize>, Vec<usize>) {
    let mut seen: HashMap<GroupKey, usize> = HashMap::new();
    let mut group_first_indices: Vec<usize> = Vec::new();
    let mut file_group_idx: Vec<usize> = Vec::with_capacity(files.len());
    for (i, f) in files.iter().enumerate() {
        let key = group_key(f);
        let idx = *seen.entry(key).or_insert_with(|| {
            let idx = group_first_indices.len();
            group_first_indices.push(i);
            idx
        });
        file_group_idx.push(idx);
    }
    (group_first_indices, file_group_idx)
}
