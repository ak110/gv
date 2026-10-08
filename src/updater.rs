//! ネットワーク更新機能
//!
//! GitHubリリースから最新版を取得し、バッチスクリプト経由でexeを置換する。

use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// GitHub Pages経由のバージョン情報URL(APIレートリミット回避)
const VERSION_URL: &str = "https://ak110.github.io/gv/version.json";

/// 更新情報
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub is_newer: bool,
}

/// GitHub Pages上のversion.jsonから最新リリース情報を取得し、バージョン比較する
pub fn check_for_update() -> Result<UpdateInfo> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    // GitHub Pages経由でバージョン情報を取得 (APIレートリミットの影響を受けない)
    let body = ureq::get(VERSION_URL)
        .header("User-Agent", &format!("gv/{current_version}"))
        .call()
        .context("バージョン情報の取得失敗")?
        .body_mut()
        .read_to_string()
        .context("レスポンス読み込み失敗")?;
    let response: serde_json::Value = serde_json::from_str(&body).context("JSONパース失敗")?;

    // tag_name から "v" プレフィクスを除去
    let tag = response["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("tag_nameが見つからない"))?;
    let latest_version = tag.strip_prefix('v').unwrap_or(tag).to_string();

    let download_url = response["download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("download_urlが見つからない"))?
        .to_string();

    let is_newer = match (
        parse_version(&current_version),
        parse_version(&latest_version),
    ) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    };

    Ok(UpdateInfo {
        current_version,
        latest_version,
        download_url,
        is_newer,
    })
}

/// ダウンロード→ZIP展開→バッチスクリプト生成→起動
/// 成功すればOk(true) を返し、呼び出し元はアプリを終了する
pub fn perform_update(info: &UpdateInfo) -> Result<bool> {
    let exe_path = crate::paths::exe_path()?;
    // ダウンロード
    let temp_dir = crate::temp_cleanup::create_temp_dir(crate::temp_cleanup::TempPurpose::Update)
        .context("更新用の一時フォルダを作成できませんでした")?;
    let result = prepare_update(info, &exe_path, &temp_dir);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    result
}

fn prepare_update(
    info: &UpdateInfo,
    exe_path: &std::path::Path,
    temp_dir: &std::path::Path,
) -> Result<bool> {
    let download_path = temp_dir.join("download");
    download_file(&info.download_url, &download_path)?;

    // ZIP展開またはそのまま使用
    let extracted = if std::path::Path::new(&info.download_url)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        extract_files_from_zip(&download_path, temp_dir)?
    } else {
        // 直接exeの場合
        let dest = temp_dir.join("update.exe");
        std::fs::rename(&download_path, &dest).context("ダウンロードファイルのリネーム失敗")?;
        ExtractedFiles {
            exe_path: dest,
            extra_files: Vec::new(),
        }
    };

    // バッチスクリプト生成・起動
    let batch_path = temp_dir.join("update.bat");
    generate_update_batch(
        &batch_path,
        &extracted.exe_path,
        exe_path,
        &extracted.extra_files,
        std::process::id(),
    )?;
    launch_batch(&batch_path)?;

    // 起動成功 → 呼び出し元がアプリを終了する
    Ok(true)
}

/// ファイルをダウンロードする
fn download_file(url: &str, dest: &std::path::Path) -> Result<()> {
    let data = ureq::get(url)
        .header("User-Agent", &format!("gv/{}", env!("CARGO_PKG_VERSION")))
        .call()
        .context("ダウンロード失敗")?
        .body_mut()
        .read_to_vec()
        .context("ダウンロードデータ読み込み失敗")?;

    std::fs::write(dest, &data).context("ダウンロードファイル書き込み失敗")?;
    Ok(())
}

/// ZIP展開結果
struct ExtractedFiles {
    /// 新しいぐらびゅ.exe のパス
    exe_path: PathBuf,
    /// exe以外のファイル (展開先パス) のリスト
    extra_files: Vec<PathBuf>,
}

/// ZIPから全ファイルを展開する
fn extract_files_from_zip(
    zip_path: &std::path::Path,
    temp_dir: &std::path::Path,
) -> Result<ExtractedFiles> {
    let file = std::fs::File::open(zip_path).context("ZIPファイルオープン失敗")?;
    let mut archive = zip::ZipArchive::new(file).context("ZIPアーカイブ読み込み失敗")?;

    let mut exe_path = None;
    let mut extra_files = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("ZIPエントリ取得失敗")?;
        if entry.is_dir() {
            continue;
        }
        // ファイル名部分のみ取得 (ZIPのパスにディレクトリが含まれる場合に対応)
        let file_name = std::path::Path::new(entry.name())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.is_empty() {
            continue;
        }

        let lower = file_name.to_lowercase();
        if lower == "ぐらびゅ.exe" {
            let dest = temp_dir.join("update.exe");
            let mut out = std::fs::File::create(&dest).context("展開先ファイル作成失敗")?;
            std::io::copy(&mut entry, &mut out).context("ZIPエントリ展開失敗")?;
            exe_path = Some(dest);
        } else {
            // exe以外のファイル (*.default.toml, README.md, LICENSE等)
            let dest = temp_dir.join(&file_name);
            let mut out = std::fs::File::create(&dest).context("展開先ファイル作成失敗")?;
            std::io::copy(&mut entry, &mut out).context("ZIPエントリ展開失敗")?;
            extra_files.push(dest);
        }
    }

    let exe_path = exe_path.ok_or_else(|| anyhow::anyhow!("ZIP内にぐらびゅ.exeが見つからない"))?;
    Ok(ExtractedFiles {
        exe_path,
        extra_files,
    })
}

/// 更新用バッチスクリプトを生成する
///
/// rename-then-replaceパターン:
/// 1. 親プロセスの終了を待機
/// 2. 実行中のexeを.oldにリネーム (Windowsはリネームを許可する)
/// 3. 新しいexeを本来の名前で配置
/// 4. 新exeを起動
/// 5. 作業フォルダをバッチ自身ごと削除する
///
/// `cleanup_old_exe()`が次回起動時に.oldを削除する。
///
/// # バッチファイル生成の制約
///
/// - エンコーディング: UTF-8 BOM + `chcp 65001` で出力する。
///   ファイル名に日本語（ぐらびゅ.exe等）を含むため CP932 では
///   DBCSトレイルバイトが cmd.exe の構文解析を破壊する
/// - 改行: `format!` が出力する LF を `replace('\n', "\r\n")` で CRLF に変換する
/// - 制御フロー: `if ( ... )` ブロック内に日本語リテラルを置かない。
///   DBCSトレイルバイトが特殊文字と誤認されるため `goto` で制御する
/// - 作業フォルダの削除は最後の行で`rmdir`と`exit 0`を連結する。
///   自身を削除した後にバッチへ戻らず、所有するcmd.exeを終了する。
fn generate_update_batch(
    batch_path: &std::path::Path,
    update_exe: &std::path::Path,
    target_exe: &std::path::Path,
    extra_files: &[PathBuf],
    pid: u32,
) -> Result<()> {
    let old_exe = target_exe.with_extension("exe.old");
    let target_dir = target_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exeの親ディレクトリ取得失敗"))?;
    let work_dir = batch_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("更新用バッチの親ディレクトリ取得失敗"))?;

    // exe以外のファイルのコピーコマンドを生成
    let extra_copy_commands = extra_files
        .iter()
        .map(|src| {
            // extra_files は perform_update 経由で「実ファイルのフルパス」が渡される前提で、
            // 末尾が `..` のような file_name() == None を返すケースは入らない
            let file_name = src
                .file_name()
                .expect("extra_filesにはファイル名を持つパスのみが渡される前提");
            let file_name = file_name.to_string_lossy();
            let dest = target_dir.join(file_name.as_ref());
            format!(
                "copy /y \"{}\" \"{}\"\nif errorlevel 1 goto extra_copy_failed",
                src.display(),
                dest.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content = format!(
        r#"@echo off
chcp 65001 >nul
title gv update

echo ぐらびゅ を更新しています...
echo.

:wait_exit
tasklist /fi "PID eq {pid}" 2>nul | find "{pid}" >nul
if %errorlevel% neq 0 goto wait_done
echo アプリケーションの終了を待機中...
timeout /t 1 /nobreak >nul
goto wait_exit
:wait_done

echo アプリケーション終了確認

:rename
if exist "{old}" del /f "{old}"
rename "{target}" "{old_name}"
if %errorlevel% equ 0 goto rename_ok
echo.
echo エラー: ぐらびゅ.exe のリネームに失敗しました。
echo アプリケーションがまだ実行中の可能性があります。
echo.
echo 何かキーを押すとリトライします...
pause >nul
goto rename
:rename_ok

echo リネーム完了

move /y "{update}" "{target}"
if %errorlevel% equ 0 goto move_ok
echo.
echo エラー: 新しい ぐらびゅ.exe の配置に失敗しました。
echo ロールバック中...
if exist "{target}" del /f "{target}"
if exist "{target}" goto rollback_failed
rename "{old}" "{target_name}"
if %errorlevel% neq 0 goto rollback_failed
echo.
echo 何かキーを押すとリトライします...
pause >nul
goto rename
:rollback_failed
echo エラー: 元の実行ファイルを復元できませんでした。
echo "{old}" を削除せず、アプリケーションを終了してから "{target}" へ戻してください。
pause >nul
exit /b 1
:move_ok

:copy_extra
{extra_copy_commands}
goto update_done

:extra_copy_failed
echo.
echo エラー: 付属ファイルのコピーに失敗しました。更新は完了していません。
echo 保存先のアクセス権や空き容量を確認し、何かキーを押すと付属ファイルを再試行します。
pause >nul
goto copy_extra

:update_done
echo.
echo 更新が完了しました。ぐらびゅ を起動します...
start "" "{target}"
cd /d "%TEMP%"
rmdir /s /q "{work}" & exit 0
"#,
        pid = pid,
        update = update_exe.display(),
        target = target_exe.display(),
        work = work_dir.display(),
        old = old_exe.display(),
        // 呼び出し元 perform_update が exe ファイルの絶対パスから組み立てるため
        // file_name() は必ず Some を返す前提
        old_name = old_exe
            .file_name()
            .expect("old_exe は target_exe を元に組み立てるためファイル名を持つ前提")
            .to_string_lossy(),
        target_name = target_exe
            .file_name()
            .expect("target_exe は crate::paths::exe_path() 由来でファイル名を持つ前提")
            .to_string_lossy(),
        extra_copy_commands = extra_copy_commands,
    );

    let content = content.replace('\n', "\r\n");
    let mut encoded = Vec::with_capacity(3 + content.len());
    encoded.extend_from_slice(b"\xEF\xBB\xBF");
    encoded.extend_from_slice(content.as_bytes());
    std::fs::write(batch_path, encoded).context("バッチスクリプト書き込み失敗")
}

/// バッチスクリプトを起動する (コンソールウィンドウ表示)
fn launch_batch(batch_path: &std::path::Path) -> Result<()> {
    use std::os::windows::process::CommandExt;

    // ジョブオブジェクトから分離を試みる (ベストエフォート)
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;
    let result = std::process::Command::new("cmd.exe")
        .args(["/k", &batch_path.display().to_string()])
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
        .spawn();

    if result.is_err() {
        // ブレイクアウェイ不可の場合はフラグなしで起動
        std::process::Command::new("cmd.exe")
            .args(["/k", &batch_path.display().to_string()])
            .spawn()
            .context("バッチスクリプト起動失敗")?;
    }
    Ok(())
}

/// バージョン文字列を (major, minor, patch) タプルに変換
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() >= 3 {
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    } else if parts.len() == 2 {
        Some((parts[0].parse().ok()?, parts[1].parse().ok()?, 0))
    } else {
        None
    }
}

/// 起動時にぐらびゅ.exe.oldが残っていれば削除を試みる
pub fn cleanup_old_exe() {
    if let Ok(exe) = crate::paths::exe_path() {
        let old = exe.with_extension("exe.old");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::neutralize_batch_line;

    #[test]
    fn parse_version_semver() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn parse_version_two_parts() {
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
    }

    #[test]
    fn parse_version_invalid() {
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1"), None);
    }

    #[test]
    fn version_comparison() {
        assert!(parse_version("1.1.0").unwrap() > parse_version("1.0.0").unwrap());
        assert!(parse_version("2.0.0").unwrap() > parse_version("1.9.9").unwrap());
        assert!(parse_version("0.2.0").unwrap() > parse_version("0.1.0").unwrap());
        assert!(parse_version("0.1.0").unwrap() <= parse_version("0.1.0").unwrap());
    }

    #[test]
    fn batch_execution_renames_and_moves_files() {
        // テスト用ディレクトリとダミーファイルを作成
        let dir = crate::test_helpers::TempDir::new("batch_exec");
        let work =
            crate::temp_cleanup::create_temp_dir_in(&dir, crate::temp_cleanup::TempPurpose::Update)
                .unwrap();

        let target = dir.join("gv.exe");
        let update = work.join("update.exe");
        std::fs::write(&target, b"OLD_CONTENT").unwrap();
        std::fs::write(&update, b"NEW_CONTENT").unwrap();

        // 存在しないPIDでバッチ生成 (wait_exitを即通過)
        let batch_path = work.join("update.bat");
        generate_update_batch(&batch_path, &update, &target, &[], u32::MAX).unwrap();

        // アプリの起動だけを無効化し、作業フォルダの削除まで実行する。
        let bytes = std::fs::read(&batch_path).unwrap();
        let bytes = neutralize_batch_line(&bytes, b"start ");
        let test_batch = work.join("update_test.bat");
        std::fs::write(&test_batch, bytes).unwrap();

        // バッチ実行
        let output = std::process::Command::new("cmd.exe")
            .args(["/c", &test_batch.display().to_string()])
            .output()
            .expect("cmd.exe実行失敗");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !work.exists(),
            "更新作業フォルダが残った: {}",
            work.display()
        );

        // ファイル操作の結果を検証
        let old = target.with_extension("exe.old");
        assert!(
            old.exists(),
            "gv.exe.old が存在するべき\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            std::fs::read(&old).unwrap(),
            b"OLD_CONTENT",
            "gv.exe.old の中身は元のexeであるべき"
        );
        assert!(
            target.exists(),
            "gv.exe が存在するべき (moveで配置される)\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"NEW_CONTENT",
            "gv.exe の中身は新しいexeであるべき"
        );
        assert!(!update.exists(), "gv_update.exe は move で消えているべき");
    }

    #[test]
    fn batch_execution_cleans_up_existing_old() {
        // .old が既に存在する場合に削除してからリネームすることを確認
        let dir = crate::test_helpers::TempDir::new("batch_old");
        let work =
            crate::temp_cleanup::create_temp_dir_in(&dir, crate::temp_cleanup::TempPurpose::Update)
                .unwrap();

        let target = dir.join("gv.exe");
        let update = work.join("update.exe");
        let old = target.with_extension("exe.old");
        std::fs::write(&target, b"CURRENT").unwrap();
        std::fs::write(&update, b"UPDATED").unwrap();
        std::fs::write(&old, b"STALE_OLD").unwrap();

        let batch_path = work.join("update.bat");
        generate_update_batch(&batch_path, &update, &target, &[], u32::MAX).unwrap();

        let bytes = std::fs::read(&batch_path).unwrap();
        let bytes = neutralize_batch_line(&bytes, b"start ");
        let test_batch = work.join("update_test.bat");
        std::fs::write(&test_batch, bytes).unwrap();

        let output = std::process::Command::new("cmd.exe")
            .args(["/c", &test_batch.display().to_string()])
            .output()
            .expect("cmd.exe実行失敗");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !work.exists(),
            "更新作業フォルダが残った: {}",
            work.display()
        );

        assert_eq!(
            std::fs::read(&old).unwrap(),
            b"CURRENT",
            "gv.exe.old は現在のexeであるべき (古い.oldは削除済み)\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"UPDATED",
            "gv.exe は新しいexeであるべき"
        );
    }

    #[test]
    fn batch_execution_copies_extra_files() {
        // exe更新と同時に追加ファイル (*.default.toml, README.md等) もコピーされることを確認
        let base_dir = crate::test_helpers::TempDir::new("batch_extra");
        // exeのあるディレクトリと、展開先の一時ディレクトリを分離
        let install_dir = base_dir.join("install");
        let temp_dir = crate::temp_cleanup::create_temp_dir_in(
            &base_dir,
            crate::temp_cleanup::TempPurpose::Update,
        )
        .unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&temp_dir).unwrap();

        let target = install_dir.join("gv.exe");
        let update = temp_dir.join("gv_update.exe");
        std::fs::write(&target, b"OLD_EXE").unwrap();
        std::fs::write(&update, b"NEW_EXE").unwrap();

        // 追加ファイルを一時ディレクトリに作成 (ZIP展開後の状態を再現)
        let extra1 = temp_dir.join("ぐらびゅ.default.toml");
        let extra2 = temp_dir.join("README.md");
        std::fs::write(&extra1, b"NEW_TOML").unwrap();
        std::fs::write(&extra2, b"NEW_README").unwrap();

        // 既存の追加ファイルも配置 (上書きされることを確認)
        std::fs::write(install_dir.join("ぐらびゅ.default.toml"), b"OLD_TOML").unwrap();
        std::fs::write(install_dir.join("README.md"), b"OLD_README").unwrap();

        let batch_path = temp_dir.join("update.bat");
        generate_update_batch(
            &batch_path,
            &update,
            &target,
            &[extra1.clone(), extra2.clone()],
            u32::MAX,
        )
        .unwrap();

        let bytes = std::fs::read(&batch_path).unwrap();
        let bytes = neutralize_batch_line(&bytes, b"start ");
        let test_batch = temp_dir.join("update_test.bat");
        std::fs::write(&test_batch, bytes).unwrap();

        let output = std::process::Command::new("cmd.exe")
            .args(["/c", &test_batch.display().to_string()])
            .output()
            .expect("cmd.exe実行失敗");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !temp_dir.exists(),
            "更新作業フォルダが残った: {}",
            temp_dir.display()
        );

        // exe が更新されていること
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"NEW_EXE",
            "gv.exe が更新されるべき\nstdout: {stdout}\nstderr: {stderr}"
        );

        // 追加ファイルがコピーされていること
        assert_eq!(
            std::fs::read(install_dir.join("ぐらびゅ.default.toml")).unwrap(),
            b"NEW_TOML",
            "ぐらびゅ.default.toml が更新されるべき\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            std::fs::read(install_dir.join("README.md")).unwrap(),
            b"NEW_README",
            "README.md が更新されるべき\nstdout: {stdout}\nstderr: {stderr}"
        );
    }

    /// 先頭・後続のコピー失敗では完了せず、付属コピーだけの再試行で完了できる。
    #[test]
    fn batch_copy_failure_stops_completion_and_retry_preserves_original_backup() {
        for failed_index in 0..2 {
            let base = crate::test_helpers::TempDir::new("update_copy_failure");
            let install = base.join("日本語 install");
            let download = crate::temp_cleanup::create_temp_dir_in(
                &base,
                crate::temp_cleanup::TempPurpose::Update,
            )
            .unwrap();
            std::fs::create_dir_all(&install).unwrap();
            let target = install.join("ぐらびゅ.exe");
            let update = download.join("update.exe");
            std::fs::write(&target, b"old exe").unwrap();
            std::fs::write(&update, b"new exe").unwrap();
            let extras = [download.join("first.toml"), download.join("second.toml")];
            for path in &extras {
                std::fs::write(path, b"new extra").unwrap();
            }
            std::fs::remove_file(&extras[failed_index]).unwrap();
            let batch = download.join("update.bat");
            generate_update_batch(&batch, &update, &target, &extras, u32::MAX).unwrap();
            let bytes = std::fs::read(&batch).unwrap();
            let bytes = neutralize_batch_line(&bytes, b"start ");
            // 利用者の入力待ちだけを置換し、失敗地点で終了して完了表示の不在を判定する。
            let failed = String::from_utf8(bytes.clone())
                .unwrap()
                .replace("pause >nul", "rem pause")
                .replace("goto copy_extra\r\n", "exit /b 1\r\n");
            let test_batch = download.join("failed.bat");
            std::fs::write(&test_batch, failed).unwrap();
            let result = std::process::Command::new("cmd.exe")
                .arg("/c")
                .arg(&test_batch)
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(1),
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(!stdout.contains("更新が完了しました"), "{stdout}");
            let old = target.with_extension("exe.old");
            assert_eq!(std::fs::read(&old).unwrap(), b"old exe");
            assert_eq!(std::fs::read(&target).unwrap(), b"new exe");
            assert!(download.exists(), "再試行用の作業フォルダを保持する");

            // 元の生成バッチの追加コピー再試行入口から続け、exeの退避を繰り返さない。
            std::fs::write(&extras[failed_index], b"new extra").unwrap();
            let original = String::from_utf8(bytes).unwrap();
            let resumed =
                original.replacen("title gv update", "title gv update\r\ngoto copy_extra", 1);
            let resumed_batch = download.join("resumed.bat");
            std::fs::write(&resumed_batch, resumed).unwrap();
            let result = std::process::Command::new("cmd.exe")
                .arg("/c")
                .arg(&resumed_batch)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stdout).contains("更新が完了しました"));
            assert_eq!(std::fs::read(&old).unwrap(), b"old exe");
            assert_eq!(std::fs::read(&target).unwrap(), b"new exe");
            for path in &extras {
                assert_eq!(
                    std::fs::read(install.join(path.file_name().unwrap())).unwrap(),
                    b"new extra"
                );
            }
            assert!(!download.exists(), "成功後に作業フォルダを回収する");
        }
    }

    /// exe配置と元exeの復元がともに失敗した場合は、旧exeを残して失敗終了する。
    #[test]
    fn batch_rollback_failure_keeps_backup_and_stops() {
        let dir = crate::test_helpers::TempDir::new("update_rollback_failure");
        let work =
            crate::temp_cleanup::create_temp_dir_in(&dir, crate::temp_cleanup::TempPurpose::Update)
                .unwrap();
        let target = dir.join("gv.exe");
        let update = work.join("missing_update.exe");
        let old = target.with_extension("exe.old");
        std::fs::write(&target, b"original exe").unwrap();
        let batch = work.join("update.bat");
        generate_update_batch(&batch, &update, &target, &[], u32::MAX).unwrap();
        let content = String::from_utf8(std::fs::read(&batch).unwrap()).unwrap();
        // 復元操作だけに失敗を注入し、その終了状態を生成バッチに判定させる。
        let content = content
            .replace(
                &format!("rename \"{}\" \"gv.exe\"", old.display()),
                "cmd /c exit 1",
            )
            .replace("pause >nul", "rem pause")
            .replace("goto rename\r\n", "exit /b 2\r\n");
        std::fs::write(&batch, content).unwrap();
        let result = std::process::Command::new("cmd.exe")
            .arg("/c")
            .arg(&batch)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(1),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            stdout.contains("元の実行ファイルを復元できませんでした"),
            "{stdout}"
        );
        assert!(!stdout.contains("更新が完了しました"), "{stdout}");
        assert_eq!(std::fs::read(&old).unwrap(), b"original exe");
        assert!(!target.exists());
        assert!(work.exists(), "復元失敗時は作業フォルダを回収しない");
    }
}
