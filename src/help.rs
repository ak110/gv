//! GUIとCLIが共有する、読み込み済みキー設定からのヘルプ生成。

use std::fmt::Write as _;

use crate::action::Action;
use crate::ui::key_config::KeyConfig;

const MAJOR_ACTIONS: &[(Action, &str)] = &[
    (Action::NavigateBack, "前の画像へ"),
    (Action::NavigateForward, "次の画像へ"),
    (Action::Navigate5Back, "5ページ前へ"),
    (Action::Navigate5Forward, "5ページ先へ"),
    (Action::Navigate50Back, "50ページ前へ"),
    (Action::Navigate50Forward, "50ページ先へ"),
    (Action::NavigateFirst, "最初へ"),
    (Action::NavigateLast, "最後へ"),
    (Action::ZoomIn, "拡大"),
    (Action::ZoomOut, "縮小"),
    (Action::DisplayAutoShrink, "自動縮小表示"),
    (Action::DisplayAutoFit, "自動縮小・拡大表示"),
    (Action::CycleAlphaBackground, "α背景切替"),
    (Action::ToggleFullscreen, "全画面表示"),
    (Action::ToggleMenuBar, "メニューバー表示/非表示"),
    (Action::ToggleFileList, "ファイルリスト表示/非表示"),
    (Action::SortNavigateBack, "ソート順で前へ"),
    (Action::SortNavigateForward, "ソート順で次へ"),
    (Action::MarkSet, "マーク設定"),
    (Action::ShowHelp, "ヘルプ表示"),
];

pub fn build_help(keys: &KeyConfig) -> String {
    let mut text = format!(
        "ぐらびゅ v{} - Windows用画像ビューアー\n\n使い方:\n  ぐらびゅ.exe [オプション] [ファイルパス]\n\nオプション:\n  --help, -h        このヘルプを表示します\n  --register        ファイル関連付け・コンテキストメニュー・送るを一括登録します\n  --unregister      一括解除します\n\n主要キーバインド:\n",
        env!("CARGO_PKG_VERSION")
    );
    for &(action, label) in MAJOR_ACTIONS {
        let keys = keys.key_labels(action);
        let assignment = if keys.is_empty() {
            "割り当てなし".to_string()
        } else {
            keys.join(" / ")
        };
        let _ = writeln!(text, "  {assignment}  {label}");
    }
    text.push_str("\n対応フォーマット:\n");
    text.push_str(&format_help());
    text.push_str("\n全キーバインドのデフォルトは ぐらびゅ.keys.default.toml を参照してください。");
    text
}

fn format_help() -> String {
    let images = crate::image::StandardDecoder::formats()
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    let archives = crate::archive::builtin_formats()
        .iter()
        .map(|(name, extensions)| format!("{name} ({})", extensions.join(", ")))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "  画像: {images}\n  ドキュメント: {}\n  アーカイブ: {archives}\n  64bit Susieプラグイン (.sph/.spi) で拡張できます\n",
        crate::extension_registry::PDF_EXTENSION
            .trim_start_matches('.')
            .to_uppercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_displays_custom_keys_in_order_and_cleared_actions() {
        let dir = crate::test_helpers::TempDir::new("help_keys");
        let path = dir.join("keys.toml");
        std::fs::write(
            &path,
            "[navigate]\nforward = \"Ctrl+Shift+F12, MiddleClick\"\nback = \"\"\n",
        )
        .unwrap();
        let text = build_help(&KeyConfig::load(Some(&path)));
        assert!(text.contains("Ctrl+Shift+F12 / ホイールクリック  次の画像へ"));
        assert!(text.contains("割り当てなし  前の画像へ"));
    }

    #[test]
    fn help_uses_loaded_bindings_and_format_definitions() {
        let keys = KeyConfig::with_defaults();
        let text = build_help(&keys);
        for &(action, _) in MAJOR_ACTIONS {
            for key in keys.key_labels(action) {
                assert!(text.contains(&key), "{action:?}: {key}");
            }
        }
        for &(name, _) in crate::image::StandardDecoder::formats() {
            assert!(text.contains(name));
        }
        for (_, extensions) in crate::archive::builtin_formats() {
            for extension in extensions {
                assert!(text.contains(extension));
            }
        }
    }
}
