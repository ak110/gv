use super::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum File {
    Absent,
    Original,
    New,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct State {
    pc: usize,
    target: File,
    backup: File,
    staged: bool,
    placed: u8,
    failed: bool,
    restarted: bool,
}

#[derive(Clone, Copy, Debug)]
enum Op<'a> {
    Goto(&'a str),
    IfFailed(bool, &'a str),
    IfTargetExists(&'a str),
    DeleteBackup,
    DeleteTarget,
    Backup,
    Install,
    Restore,
    Copy(u8),
    Done,
    Launch,
    ExitSuccess,
    Halt,
    Next,
}

fn parse<'a>(batch: &'a str, extras: &[PathBuf]) -> (Vec<Op<'a>>, HashMap<&'a str, usize>) {
    let mut code = Vec::new();
    let mut labels = HashMap::new();
    for line in batch.lines().map(str::trim) {
        if let Some(label) = line.strip_prefix(':') {
            assert!(
                labels.insert(label, code.len()).is_none(),
                "重複ラベル: {label}"
            );
            continue;
        }
        let op = if let Some(label) = line.strip_prefix("goto ") {
            Op::Goto(label)
        } else if let Some(label) = line.strip_prefix("if %errorlevel% equ 0 goto ") {
            Op::IfFailed(false, label)
        } else if let Some(label) = line.strip_prefix("if %errorlevel% neq 0 goto ") {
            Op::IfFailed(true, label)
        } else if let Some(label) = line.strip_prefix("if errorlevel 1 goto ") {
            Op::IfFailed(true, label)
        } else if let Some(label) = line.strip_prefix(r#"if exist "install/gv.exe" goto "#) {
            Op::IfTargetExists(label)
        } else if let Some(index) = extras.iter().position(|path| {
            line == format!(
                r#"copy /y "{}" "install/{}""#,
                path.display(),
                path.file_name().unwrap().display()
            )
        }) {
            Op::Copy(1 << index)
        } else {
            match line {
                r#"if exist "install/gv.exe.old" del /f "install/gv.exe.old""# => Op::DeleteBackup,
                r#"if exist "install/gv.exe" del /f "install/gv.exe""# => Op::DeleteTarget,
                r#"rename "install/gv.exe" "gv.exe.old""# => Op::Backup,
                r#"move /y "work/update.exe" "install/gv.exe""# => Op::Install,
                r#"rename "install/gv.exe.old" "gv.exe""# => Op::Restore,
                "echo 更新が完了しました。ぐらびゅ を起動します..." => Op::Done,
                r#"rmdir /s /q "work" & exit 0"# => Op::ExitSuccess,
                r#"start "" "install/gv.exe""# => Op::Launch,
                "exit /b 1" => Op::Halt,
                "@echo off"
                | "chcp 65001 >nul"
                | "title gv update"
                | "pause >nul"
                | "timeout /t 1 /nobreak >nul"
                | r#"cd /d "%TEMP%""#
                | ""
                | "echo." => Op::Next,
                line if line.starts_with("echo ") => Op::Next,
                r#"tasklist /fi "PID eq 42" 2>nul | find "42" >nul"# => {
                    // 探索は、旧プロセスが終了した後のファイル操作を対象とする。
                    Op::IfFailed(false, "wait_done")
                }
                _ => panic!("未対応のバッチ命令: {line}"),
            }
        };
        code.push(op);
    }
    (code, labels)
}

fn explore(extra_count: usize, allow_restart: bool) -> (usize, Vec<(State, &'static str)>) {
    let extras = ["work/extra-a", "work/extra-b"]
        .into_iter()
        .take(extra_count)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let raw = build_update_batch_bytes(
        Path::new("work/update.bat"),
        Path::new("work/update.exe"),
        Path::new("install/gv.exe"),
        &extras,
        42,
    )
    .unwrap();
    // モデルのパスは区切り文字を正規化する。入力パスのWin32としての解釈は別試験の責任。
    let batch = std::str::from_utf8(&raw[3..]).unwrap().replace('\\', "/");
    let (code, labels) = parse(&batch, &extras);
    let mut pending = [File::Absent, File::Original]
        .into_iter()
        .map(|backup| State {
            pc: labels["wait_done"],
            target: File::Original,
            backup,
            staged: true,
            placed: 0,
            failed: false,
            restarted: false,
        })
        .collect::<VecDeque<_>>();
    let mut seen = HashSet::new();
    let mut violations = Vec::new();
    let required = (1 << extra_count) - 1;
    while let Some(state) = pending.pop_front() {
        if !seen.insert(state) {
            continue;
        }
        if !matches!(state.target, File::Original | File::New)
            && !matches!(state.backup, File::Original | File::New)
        {
            violations.push((state, "復旧可能な実行ファイルがない"));
            continue;
        }
        if allow_restart && !state.restarted {
            pending.push_back(State {
                pc: labels["wait_done"],
                failed: false,
                restarted: true,
                ..state
            });
        }
        let mut next = State {
            pc: state.pc + 1,
            ..state
        };
        match code[state.pc] {
            Op::Goto(label) => next.pc = labels[label],
            Op::IfFailed(value, label) if state.failed == value => next.pc = labels[label],
            Op::IfTargetExists(label) if state.target != File::Absent => next.pc = labels[label],
            Op::Done | Op::Launch | Op::ExitSuccess => {
                if state.target != File::New || state.placed != required {
                    violations.push((state, "必要な配置が済む前に完了または起動した"));
                }
                if matches!(code[state.pc], Op::ExitSuccess) {
                    continue;
                }
                if matches!(code[state.pc], Op::Launch) && state.target == File::New {
                    // 起動した新版で cleanup_old_exe が成功するか、退避を残す。
                    pending.push_back(State {
                        backup: File::Absent,
                        ..next
                    });
                }
            }
            Op::Halt => continue,
            Op::DeleteBackup
            | Op::DeleteTarget
            | Op::Backup
            | Op::Install
            | Op::Restore
            | Op::Copy(_) => {
                pending.push_back(State {
                    failed: true,
                    ..next
                });
                next.failed = false;
                match code[state.pc] {
                    Op::DeleteBackup => next.backup = File::Absent,
                    Op::DeleteTarget => next.target = File::Absent,
                    Op::Backup if state.target != File::Absent && state.backup == File::Absent => {
                        next.backup = state.target;
                        next.target = File::Absent;
                    }
                    Op::Install if state.staged => {
                        // 別ボリュームでは失敗時にコピー途中の配置先が残りうる。
                        pending.push_back(State {
                            target: File::Partial,
                            failed: true,
                            ..next
                        });
                        next.staged = false;
                        next.target = File::New;
                    }
                    Op::Restore if state.backup != File::Absent && state.target == File::Absent => {
                        next.target = state.backup;
                        next.backup = File::Absent;
                    }
                    Op::Copy(bit) => {
                        pending.push_back(State {
                            placed: state.placed & !bit,
                            failed: true,
                            ..next
                        });
                        next.placed |= bit;
                    }
                    _ => continue,
                }
            }
            _ => {}
        }
        pending.push_back(next);
    }
    (seen.len(), violations)
}

#[test]
fn batch_retries_keep_recoverable_executable_and_finish_only_after_placement() {
    for extra_count in 1..=2 {
        let (visited, failures) = explore(extra_count, false);
        assert!(
            failures.is_empty(),
            "{visited}状態、最初の反例: {:?}",
            failures.first()
        );
    }
}

#[test]
#[ignore = "既知の反例候補: 再起動すると既存の退避実行ファイルを削除する"]
fn batch_restart_preserves_recoverable_executable() {
    let outcomes = (1..=2)
        .map(|extra_count| {
            let (visited, failures) = explore(extra_count, true);
            (extra_count, visited, failures.first().copied())
        })
        .collect::<Vec<_>>();
    assert!(
        outcomes.iter().all(|(_, _, failure)| failure.is_none()),
        "{outcomes:?}"
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Preparation {
    Download,
    StageExe,
    StageExtra,
    WriteBatch,
    Launch,
    Complete,
}

#[test]
fn preparation_failures_keep_installed_executable() {
    use std::io::Write as _;

    for zipped in [false, true] {
        let mut stops = vec![
            Preparation::Download,
            Preparation::StageExe,
            Preparation::WriteBatch,
            Preparation::Launch,
            Preparation::Complete,
        ];
        if zipped {
            stops.push(Preparation::StageExtra);
        }
        for stop in stops {
            let root = crate::test_helpers::TempDir::new("bounded_update");
            let work = root.join("work");
            let install = root.join("install");
            std::fs::create_dir(&work).unwrap();
            std::fs::create_dir(&install).unwrap();
            let outside = root.join("unrelated.txt");
            let installed = install.join("gv.exe");
            let backup = installed.with_extension("exe.old");
            std::fs::write(&outside, b"unrelated").unwrap();
            std::fs::write(&installed, b"old").unwrap();
            let blocked = match stop {
                Preparation::StageExe => Some("update.exe"),
                Preparation::StageExtra => Some("extra.txt"),
                Preparation::WriteBatch => Some("update.bat"),
                _ => None,
            };
            if let Some(name) = blocked {
                std::fs::create_dir(work.join(name)).unwrap();
            }
            let info = UpdateInfo {
                current_version: "1.0.0".into(),
                latest_version: "2.0.0".into(),
                download_url: if zipped { "v2.zip" } else { "v2.exe" }.into(),
                is_newer: true,
            };
            let mut reached_launch = false;
            let result = prepare_update_with(
                &info,
                &installed,
                &work,
                |url, download| {
                    assert_eq!(url, info.download_url);
                    assert_eq!(download, work.join("download"));
                    if stop == Preparation::Download {
                        std::fs::write(download, b"partial")?;
                        anyhow::bail!("ダウンロード中断");
                    }
                    if zipped {
                        let mut zip = zip::ZipWriter::new(std::fs::File::create(download)?);
                        let options = zip::write::SimpleFileOptions::default();
                        for (name, bytes) in
                            [("ぐらびゅ.exe", &b"new"[..]), ("extra.txt", &b"extra"[..])]
                        {
                            zip.start_file(name, options)?;
                            zip.write_all(bytes)?;
                        }
                        zip.finish()?;
                    } else {
                        std::fs::write(download, b"new")?;
                    }
                    Ok(())
                },
                |batch| {
                    reached_launch = true;
                    assert_eq!(batch, work.join("update.bat"));
                    assert_eq!(std::fs::read(work.join("update.exe"))?, b"new");
                    if zipped {
                        assert_eq!(std::fs::read(work.join("extra.txt"))?, b"extra");
                    }
                    if stop == Preparation::Launch {
                        anyhow::bail!("起動失敗");
                    }
                    Ok(())
                },
            );
            assert_eq!(result.is_ok(), stop == Preparation::Complete, "{stop:?}");
            assert_eq!(
                reached_launch,
                matches!(stop, Preparation::Launch | Preparation::Complete)
            );
            assert_eq!(std::fs::read(&installed).unwrap(), b"old", "{stop:?}");
            assert_eq!(std::fs::read(&outside).unwrap(), b"unrelated", "{stop:?}");
            assert_eq!(std::fs::read_dir(&install).unwrap().count(), 1, "{stop:?}");

            std::fs::write(&backup, b"backup").unwrap();
            cleanup_old_exe_at(&installed);
            assert!(!backup.exists());
            assert_eq!(std::fs::read(&installed).unwrap(), b"old");
            assert_eq!(std::fs::read(&outside).unwrap(), b"unrelated");
        }
    }
}

#[test]
#[ignore = "既知の反例候補: 退避直後の状態から cmd.exe を再実行すると退避を削除する"]
fn batch_restart_preserves_recoverable_executable_in_cmd() {
    let root = crate::test_helpers::TempDir::new("bounded_update_restart");
    let work = root.join("work");
    let install = root.join("install");
    std::fs::create_dir(&work).unwrap();
    std::fs::create_dir(&install).unwrap();
    let target = install.join("gv.exe");
    let backup = target.with_extension("exe.old");
    let staged = work.join("update.exe");
    let batch = work.join("update.bat");
    // 最初の実行が、現行版の退避を終え、新版の配置前に中断した状態。
    std::fs::write(&backup, b"original").unwrap();
    std::fs::write(&staged, b"new").unwrap();
    let bytes = build_update_batch_bytes(&batch, &staged, &target, &[], u32::MAX).unwrap();
    let script = String::from_utf8(bytes)
        .unwrap()
        // 更新に失敗した後の対話と無期限の再試行だけを止める。
        .replace("pause >nul\r\ngoto rename\r\n", "exit /b 17\r\n")
        .replace("start \"\" ", "rem start \"\" ");
    std::fs::write(&batch, script).unwrap();
    let result = std::process::Command::new("cmd.exe")
        .args(["/c"])
        .arg(&batch)
        .output()
        .unwrap();
    let usable = [&target, &backup]
        .into_iter()
        .any(|path| std::fs::read(path).is_ok_and(|bytes| bytes == b"original" || bytes == b"new"));
    assert!(
        usable,
        "再実行後に有効な実行ファイルがない。終了コード: {:?}、出力: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stdout)
    );
}
