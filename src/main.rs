#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// 本番から到達しないコードを残さないため、非テストのビルドでは未使用を誤りにし、
// 局所の`allow(dead_code)`による抑制も拒否する。テストビルドはテストハーネスが生成する
// `main`へ`allow(dead_code)`を付けるため、`forbid`と衝突しないよう対象から外す。
// テスト専用のAPIは`#[cfg(test)]`で限定する。
#![cfg_attr(not(test), forbid(dead_code))]

mod action;
mod app;
mod archive;
mod bookmark;
mod clipboard;
mod config;
mod document;
mod extension_registry;
mod file_info;
mod file_list;
mod filter;
mod filter_spec;
mod help;
mod image;
mod paths;
mod pdf_renderer;
mod persistent_filter;
mod prefetch;
mod render;
mod selection;
mod shell;
mod susie;
mod temp_cleanup;
mod ui;
mod updater;
mod util;

#[cfg(test)]
mod test_helpers;

use std::path::PathBuf;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

fn main() {
    if std::env::args().nth(1).is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "--help" | "-h" | "--register" | "--unregister"
        )
    }) {
        // コンソールが無い起動では接続に失敗し、GUIと同じく出力先を持たない。
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }

    // CLIフラグの分岐 (DPI/COM初期化前に処理: 副作用不要なもの)
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    // DPI awareness設定 (COM初期化より先に呼ぶ必要がある)
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // COM初期化
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() } {
        exit_with_startup_error("COMの初期化に失敗しました", &format!("{e:#}"));
    }

    // 起動時マイグレーション・クリーンアップ
    migrate_old_filenames();
    temp_cleanup::cleanup_orphaned_temp_dirs();
    updater::cleanup_old_exe();

    // CLI分岐 (--register / --unregister: COM初期化後に実行)
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--register" => {
                finish_shell_command(shell::register_all(), "シェル統合の登録", "--register");
                return;
            }
            "--unregister" => {
                finish_shell_command(shell::unregister_all(), "シェル統合の解除", "--unregister");
                return;
            }
            _ => {}
        }
    }

    // 設定ファイル読み込み
    let config = config::Config::load();

    // コマンドライン引数からファイルパスを収集 (--で始まるフラグは除外)
    let initial_files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|arg| !arg.to_str().is_some_and(|s| s.starts_with("--")))
        .map(PathBuf::from)
        .collect();

    // メインウィンドウ作成
    // _appはメッセージループ中に生存する必要がある (Box<WindowState>のドロップ防止)
    let _app = match app::AppWindow::create(config, &initial_files) {
        Ok(app) => app,
        Err(e) => exit_with_startup_error("ウィンドウの作成に失敗しました", &format!("{e:#}")),
    };

    // メッセージループ
    let exit_code = ui::window::run_message_loop();
    std::process::exit(exit_code);
}

/// 起動の失敗を理由と次の操作とともにダイアログで示し、非0で終了する
///
/// release版はウィンドウサブシステムで標準エラーの出力先が無いため、ダイアログで伝える。
fn exit_with_startup_error(summary: &str, detail: &str) -> ! {
    util::show_error_box(
        windows::Win32::Foundation::HWND::default(),
        "ぐらびゅ",
        summary,
        "ぐらびゅをもう一度起動してください。繰り返し失敗する場合は、\
         ホームページ（https://github.com/ak110/gv）のIssuesへこの内容を添えて報告してください",
        Some(detail),
    );
    std::process::exit(1);
}

/// `--register`・`--unregister`の失敗を理由と次の操作とともにコンソールへ出し、非0で終了する
fn finish_shell_command(result: anyhow::Result<()>, operation: &str, flag: &str) {
    if let Err(e) = result {
        eprintln!(
            "{}",
            util::format_error_text(
                &format!("{operation}に失敗しました"),
                &format!(
                    "レジストリ（HKEY_CURRENT_USER）への書き込みを妨げるセキュリティソフトなどが無いか確認し、\
                     「ぐらびゅ.exe {flag}」を再実行してください"
                ),
                Some(&format!("{e:#}")),
            )
        );
        std::process::exit(1);
    }
}

/// 旧ファイル名 (gv3.*) を新ファイル名 (ぐらびゅ.*) にリネームする
fn migrate_old_filenames() {
    let Ok(dir) = crate::paths::exe_dir() else {
        return;
    };
    let migrations = [
        ("gv3.toml", "ぐらびゅ.toml"),
        ("gv3.keys.toml", "ぐらびゅ.keys.toml"),
        ("gv3.default.toml", "ぐらびゅ.default.toml"),
        ("gv3.keys.default.toml", "ぐらびゅ.keys.default.toml"),
    ];
    for (old, new) in &migrations {
        let old_path = dir.join(old);
        let new_path = dir.join(new);
        if old_path.exists() && !new_path.exists() {
            let _ = std::fs::rename(&old_path, &new_path);
        }
    }
}

fn print_help() {
    let key_path = crate::paths::key_config_path().ok();
    let keys = ui::key_config::KeyConfig::load(key_path.as_deref());
    println!("{}", help::build_help(&keys));
}
