use anyhow::{Context as _, Result};
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Foundation::HWND;

/// 内容を完成させてから保存先へ反映する。書き込み・置換に失敗しても既存内容を残す。
///
/// 同一フォルダで作成するため、置換が別ボリュームへのコピーにならない。
/// Windowsではstd::fs::renameが既存ファイルの置換を行い、共有違反・読み取り専用なら失敗する。
pub fn save_atomic(
    path: &Path,
    write_contents: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    let mut pending = PendingSave::create(path)?;
    let mut file = pending.file.take().expect("新規保存ファイルが存在する");
    let result = write_contents(&mut file).and_then(|()| file.sync_all().map_err(Into::into));
    // Windowsでの後始末・置換の前に必ずハンドルを閉じる。
    drop(file);
    result?;
    std::fs::rename(&pending.path, path)
        .with_context(|| format!("保存先を置換できませんでした: {}", path.display()))
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    save_atomic(path, |file| file.write_all(data).map_err(Into::into))
}

pub fn copy_atomic(source: &Path, destination: &Path) -> Result<()> {
    let mut source_file = File::open(source)
        .with_context(|| format!("コピー元を開けませんでした: {}", source.display()))?;
    save_atomic(destination, |file| {
        std::io::copy(&mut source_file, file)?;
        file.set_permissions(source_file.metadata()?.permissions())?;
        Ok(())
    })
}

struct PendingSave {
    path: PathBuf,
    file: Option<File>,
}

impl PendingSave {
    fn create(destination: &Path) -> Result<Self> {
        static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let directory = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        loop {
            let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".gv-save-{}-{sequence}.tmp", std::process::id()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "保存用ファイルを作成できませんでした: {}",
                            directory.display()
                        )
                    });
                }
            }
        }
    }
}

impl Drop for PendingSave {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn delete_to_recycle_bin(hwnd: HWND, paths: &[&Path]) -> Result<bool> {
    #[cfg(test)]
    if let Some(result) = test_driver::capture_operation("delete", paths) {
        return Ok(result);
    }
    if paths.is_empty() {
        return Ok(false);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO | FOFLAG_WANTNUKEWARNING)?;
        for path in paths {
            let item = create_shell_item_strict(path)?;
            op.DeleteItem(&item, None)
                .context("DeleteItemの設定に失敗しました")?;
        }
        perform_operations(&op, "ファイル削除に失敗しました")
    }
}

pub fn move_files(hwnd: HWND, paths: &[&Path], dest: &Path) -> Result<bool> {
    #[cfg(test)]
    if let Some(result) = test_driver::capture_operation("move", paths) {
        return Ok(result);
    }
    if paths.is_empty() {
        return Ok(false);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO)?;
        let dest_folder = create_shell_item_strict(dest)?;
        for path in paths {
            let item = create_shell_item_strict(path)?;
            op.MoveItem(&item, &dest_folder, None, None)
                .context("MoveItemの設定に失敗しました")?;
        }
        perform_operations(
            &op,
            &format!("ファイル移動に失敗しました\n  dest: {}", dest.display()),
        )
    }
}

pub fn move_single_file(hwnd: HWND, src: &Path, dest: &Path) -> Result<bool> {
    #[cfg(test)]
    if let Some(result) = test_driver::capture_operation("move_single", &[src]) {
        return Ok(result);
    }
    unsafe {
        let op = create_file_operation(hwnd, FOFLAG_ALLOWUNDO)?;
        let src_item = create_shell_item_strict(src)?;
        let dest_parent = dest
            .parent()
            .context("移動先の親ディレクトリが存在しない")?;
        let dest_folder = create_shell_item_strict(dest_parent)?;
        let new_name = dest.file_name().context("移動先のファイル名が存在しない")?;
        let new_name_wide: Vec<u16> = new_name.encode_wide().chain(std::iter::once(0)).collect();
        op.MoveItem(
            &src_item,
            &dest_folder,
            windows::core::PCWSTR(new_name_wide.as_ptr()),
            None,
        )
        .context("MoveItemの設定に失敗しました")?;
        perform_operations(
            &op,
            &format!(
                "ファイル移動に失敗しました\n  src: {}\n  dest: {}",
                src.display(),
                dest.display()
            ),
        )
    }
}

use windows::Win32::UI::Shell::FILEOPERATION_FLAGS;

const FOFLAG_ALLOWUNDO: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x0040);
const FOFLAG_WANTNUKEWARNING: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(0x4000);

unsafe fn create_file_operation(
    hwnd: HWND,
    flags: FILEOPERATION_FLAGS,
) -> Result<windows::Win32::UI::Shell::IFileOperation> {
    unsafe {
        let op: windows::Win32::UI::Shell::IFileOperation =
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::UI::Shell::FileOperation,
                None,
                windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
            )
            .context("IFileOperation作成失敗")?;
        op.SetOwnerWindow(hwnd)?;
        op.SetOperationFlags(flags)?;
        Ok(op)
    }
}

unsafe fn perform_operations(
    op: &windows::Win32::UI::Shell::IFileOperation,
    error_msg: &str,
) -> Result<bool> {
    unsafe {
        op.PerformOperations().context(error_msg.to_string())?;
        let aborted = op.GetAnyOperationsAborted()?;
        Ok(!aborted.as_bool())
    }
}

fn create_shell_item_strict(path: &Path) -> Result<windows::Win32::UI::Shell::IShellItem> {
    let path = crate::util::strip_extended_length_prefix(path);
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        windows::Win32::UI::Shell::SHCreateItemFromParsingName(
            windows::core::PCWSTR(wide.as_ptr()),
            None,
        )
        .with_context(|| format!("ShellItem作成失敗: {}", path.display()))
    }
}

#[cfg(test)]
pub(crate) mod test_driver {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    type CapturedOperations = Vec<(&'static str, Vec<PathBuf>)>;

    #[derive(Default)]
    struct Driver {
        selected: Option<PathBuf>,
        operations: CapturedOperations,
        allow_real_moves: bool,
    }

    thread_local! {
        static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    }

    pub(crate) fn with_selected_path(
        selected: PathBuf,
        action: impl FnOnce(),
    ) -> CapturedOperations {
        with_driver(selected, false, action)
    }

    pub(crate) fn with_selected_path_and_real_moves(
        selected: PathBuf,
        action: impl FnOnce(),
    ) -> CapturedOperations {
        with_driver(selected, true, action)
    }

    fn with_driver(
        selected: PathBuf,
        allow_real_moves: bool,
        action: impl FnOnce(),
    ) -> CapturedOperations {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                DRIVER.with(|slot| slot.borrow_mut().take());
            }
        }
        DRIVER.with(|slot| {
            assert!(
                slot.borrow_mut()
                    .replace(Driver {
                        selected: Some(selected),
                        operations: Vec::new(),
                        allow_real_moves,
                    })
                    .is_none()
            );
        });
        let _reset = Reset;
        action();
        DRIVER.with(|slot| std::mem::take(&mut slot.borrow_mut().as_mut().unwrap().operations))
    }

    pub(crate) fn selected_path() -> Option<PathBuf> {
        DRIVER.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|driver| driver.selected.clone())
        })
    }

    pub(super) fn capture_operation(kind: &'static str, paths: &[&Path]) -> Option<bool> {
        DRIVER.with(|slot| {
            let mut slot = slot.borrow_mut();
            let driver = slot.as_mut()?;
            driver
                .operations
                .push((kind, paths.iter().map(|path| path.to_path_buf()).collect()));
            if driver.allow_real_moves && matches!(kind, "move" | "move_single") {
                None
            } else {
                Some(false)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;

    #[test]
    fn atomic_save_preserves_existing_content_on_failure_and_recovers() {
        let dir = crate::test_helpers::TempDir::new("atomic_save");
        let destination = dir.join("保存先.dat");
        std::fs::write(&destination, b"original").unwrap();

        let err = save_atomic(&destination, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into())
        })
        .unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&destination)
            .unwrap();
        let err = write_atomic(&destination, b"replacement").unwrap_err();
        drop(locked);
        assert!(err.to_string().starts_with("保存先を置換できませんでした:"));
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().raw_os_error(),
            Some(windows::Win32::Foundation::ERROR_SHARING_VIOLATION.0 as i32)
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        write_atomic(&destination, b"replacement").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"replacement");

        let copy = dir.join("複製.dat");
        copy_atomic(&destination, &copy).unwrap();
        copy_atomic(&copy, &copy).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        let err = copy_atomic(&dir.join("missing"), &copy).unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        let original_permissions = std::fs::metadata(&destination).unwrap().permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        std::fs::set_permissions(&destination, readonly_permissions).unwrap();
        let locked_copy = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&copy)
            .unwrap();
        assert!(copy_atomic(&destination, &copy).is_err());
        drop(locked_copy);
        std::fs::set_permissions(&destination, original_permissions).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"replacement");
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        let mut expected = vec![destination.file_name().unwrap(), copy.file_name().unwrap()];
        expected.sort();
        assert_eq!(names, expected);
    }
}
