use std::path::PathBuf;

use crate::archive::ArchiveImageEntry;

#[derive(Debug)]
pub enum ZipBuffer {
    Mmap(memmap2::Mmap),
    Memory(Vec<u8>),
}

impl AsRef<[u8]> for ZipBuffer {
    fn as_ref(&self) -> &[u8] {
        match self {
            ZipBuffer::Mmap(m) => m,
            ZipBuffer::Memory(v) => v,
        }
    }
}

#[derive(Debug)]
pub enum DocumentEvent {
    ImageReady,
    FileListChanged,
    NavigationChanged { index: usize },
    Error(ErrorNotice),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNotice {
    pub summary: String,
    pub next_action: &'static str,
    pub detail: Option<String>,
}

impl ErrorNotice {
    pub fn new(summary: impl Into<String>, next_action: &'static str) -> Self {
        Self {
            summary: summary.into(),
            next_action,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

pub(super) struct ExtractedDirectory(pub(super) PathBuf);

impl Drop for ExtractedDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) enum ContainerResult {
    Pdf {
        path: PathBuf,
        page_count: u32,
    },
    Zip {
        path: PathBuf,
        buffer: ZipBuffer,
        entries: Vec<ArchiveImageEntry>,
    },
    TempExtracted {
        path: PathBuf,
        temp_dir: ExtractedDirectory,
        entries: Vec<crate::archive::ExtractedEntry>,
    },
    Error {
        path: PathBuf,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerState {
    Pending,
    InFlight,
    Expanded,
}

pub(super) enum ContainerExpandEvent {
    Expanded {
        container_path: PathBuf,
        result: ContainerResult,
        generation: u64,
    },
    AllDone {
        generation: u64,
    },
}
