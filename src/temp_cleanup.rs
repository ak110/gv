//! gvの一時資源の生成と、異常終了後に残った資源の回収。
//!
//! 作成したパスは各利用側が所有し、正常終了時に回収する。

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER};
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempPurpose {
    Archive,
    Paste,
    Update,
}

impl TempPurpose {
    fn name(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Paste => "paste",
            Self::Update => "update",
        }
    }
}

static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn next_path(root: &Path, purpose: TempPurpose, extension: &str) -> PathBuf {
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        "gv_{}_{}_{sequence}{extension}",
        purpose.name(),
        std::process::id()
    ))
}

/// 既存の一時フォルダと共有せず、新しいフォルダを作成する。
pub fn create_temp_dir(purpose: TempPurpose) -> io::Result<PathBuf> {
    create_temp_dir_in(&std::env::temp_dir(), purpose)
}

pub fn create_temp_dir_in(root: &Path, purpose: TempPurpose) -> io::Result<PathBuf> {
    loop {
        let path = next_path(root, purpose, "");
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// 貼り付けのPNGを排他的に新規作成する。
pub fn create_paste_file(root: &Path) -> io::Result<(PathBuf, File)> {
    loop {
        let path = next_path(root, TempPurpose::Paste, ".png");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// 自プロセスと生存中のプロセスを残し、孤立した全用途の資源を削除する。
pub fn cleanup_orphaned_temp_dirs() {
    cleanup_in(&std::env::temp_dir(), std::process::id(), is_process_alive);
}

fn cleanup_in(root: &Path, my_pid: u32, is_alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Some((_, pid)) = parse_temp_name(name_str) else {
            continue;
        };

        // 自プロセスはスキップ
        if pid == my_pid {
            continue;
        }

        // PID生存チェック
        if is_alive(pid) {
            continue;
        }

        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        } else {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 旧アーカイブ・更新の形式も含め、用途とPIDを取得する。
fn parse_temp_name(name: &str) -> Option<(TempPurpose, u32)> {
    for purpose in [
        TempPurpose::Archive,
        TempPurpose::Paste,
        TempPurpose::Update,
    ] {
        let prefix = format!("gv_{}_", purpose.name());
        if let Some(rest) = name.strip_prefix(&prefix) {
            return Some((purpose, rest.split('_').next()?.parse().ok()?));
        }
    }
    let rest = name.strip_prefix("gv3_archive_")?;
    Some((TempPurpose::Archive, rest.split('_').next()?.parse().ok()?))
}

/// Win32 OpenProcess でプロセスの生存をチェック
fn is_process_alive(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            // 存在しないPIDだけを回収し、アクセス拒否などの判定不能は保護する。
            Err(error) => {
                error.code() != windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pid_valid() {
        assert_eq!(
            parse_temp_name("gv_archive_12345_1700000000000"),
            Some((TempPurpose::Archive, 12345))
        );
    }

    #[test]
    fn parse_pid_legacy_prefix() {
        // 旧プレフィクス gv3_archive_ もサポート
        assert_eq!(
            parse_temp_name("gv3_archive_12345_1700000000000"),
            Some((TempPurpose::Archive, 12345))
        );
    }

    #[test]
    fn parse_pid_no_timestamp() {
        assert_eq!(
            parse_temp_name("gv_archive_99"),
            Some((TempPurpose::Archive, 99))
        );
    }

    #[test]
    fn parse_pid_invalid_prefix() {
        assert_eq!(parse_temp_name("gv3_other_12345_100"), None);
    }

    #[test]
    fn parse_pid_non_numeric() {
        assert_eq!(parse_temp_name("gv_archive_abc_100"), None);
    }

    #[test]
    fn current_process_is_alive() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn dead_process_is_not_alive() {
        // PID=0はSystem Idle Process、通常OpenProcessでアクセス拒否される
        // 最大値に近いPIDは通常存在しない
        assert!(!is_process_alive(u32::MAX));
    }

    #[test]
    fn created_resources_are_unique_and_keep_existing_contents() {
        use std::io::Write as _;
        let root = crate::test_helpers::TempDir::new("temp_unique");
        for purpose in [TempPurpose::Archive, TempPurpose::Update] {
            let first = create_temp_dir_in(&root, purpose).unwrap();
            let second = create_temp_dir_in(&root, purpose).unwrap();
            assert_ne!(first, second);
            assert!(first.is_dir() && second.is_dir());
        }
        let (first, mut file) = create_paste_file(&root).unwrap();
        file.write_all(b"first").unwrap();
        drop(file);
        let (second, file) = create_paste_file(&root).unwrap();
        drop(file);
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first).unwrap(), b"first");
    }

    #[test]
    fn cleanup_removes_all_dead_resources_and_keeps_live_and_own_resources() {
        let root = crate::test_helpers::TempDir::new("temp_cleanup");
        let own = std::process::id();
        let dead = u32::MAX;
        let live = own.wrapping_add(1);
        let mut removed = Vec::new();
        for (name, directory) in [
            (format!("gv_archive_{dead}_1"), true),
            (format!("gv_paste_{dead}_2.png"), false),
            (format!("gv_update_{dead}_3"), true),
            (format!("gv3_archive_{dead}_4"), true),
            (format!("gv_update_{dead}"), true),
        ] {
            let path = root.join(name);
            if directory {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, b"orphan").unwrap();
            }
            removed.push(path);
        }
        let mut preserved = Vec::new();
        for pid in [own, live] {
            for purpose in ["archive", "paste", "update"] {
                let path = root.join(format!("gv_{purpose}_{pid}_0"));
                std::fs::write(&path, b"owned").unwrap();
                preserved.push(path);
            }
        }
        let unrelated = root.join("other_4294967295_0");
        std::fs::write(&unrelated, b"unrelated").unwrap();
        preserved.push(unrelated);
        cleanup_in(&root, own, |pid| pid == live);
        assert!(removed.iter().all(|path| !path.exists()));
        assert!(preserved.iter().all(|path| path.exists()));
    }
}
