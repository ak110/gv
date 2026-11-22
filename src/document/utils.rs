use std::path::Path;

pub(super) fn sort_paths_natural(paths: &mut [std::path::PathBuf]) {
    use crate::file_list::compare_paths_natural;
    paths.sort_by(|a, b| {
        let ak = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let bk = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        compare_paths_natural(ak, bk)
    });
}

/// Windows のリパースポイント (シンボリックリンク・ジャンクション等) か判定する。
/// `FileType::is_symlink()` はジャンクションを確実に識別できないため、
/// `MetadataExt::file_attributes()` で `FILE_ATTRIBUTE_REPARSE_POINT` ビットを直接検査する。
pub(super) fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

pub(super) fn collect_folder_files_recursive(folder: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    walk_folder_recursive(folder, &mut out);
    out
}

fn walk_folder_recursive(folder: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut subdirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if is_reparse_point(&meta) {
            continue;
        }
        if meta.is_file() {
            files.push(path);
        } else if meta.is_dir() {
            subdirs.push(path);
        }
    }
    sort_paths_natural(&mut files);
    sort_paths_natural(&mut subdirs);
    out.extend(files);
    for subdir in subdirs {
        walk_folder_recursive(&subdir, out);
    }
}

pub(super) fn set_current_thread_background_io_priority() -> windows::core::Result<()> {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
    };
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }
}

pub(super) fn build_expansion_pool() -> anyhow::Result<rayon::ThreadPool> {
    use anyhow::Context;
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .thread_name(|_| "bg-expansion".to_string())
        .start_handler(|_idx| {
            if let Err(e) = set_current_thread_background_io_priority() {
                eprintln!("背景展開スレッドの I/O 優先度設定に失敗: {e}");
            }
        })
        .build()
        .context("背景展開用 rayon プールの構築に失敗")
}
