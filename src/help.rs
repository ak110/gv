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
        let dir = crate::test_helpers::TempDir::new("help_formats");
        let path = dir.join("keys.toml");
        std::fs::write(
            &path,
            "[navigate]\nforward = \"Ctrl+F12\"\nback = \"Shift+F11\"\n",
        )
        .unwrap();
        let text = build_help(&KeyConfig::load(Some(&path)));
        let (_, sections) = text.split_once("主要キーバインド:\n").unwrap();
        let (bindings, formats) = sections.split_once("\n対応フォーマット:\n").unwrap();
        assert!(
            bindings
                .lines()
                .any(|line| line == "  Ctrl+F12  次の画像へ")
        );
        assert!(
            bindings
                .lines()
                .any(|line| line == "  Shift+F11  前の画像へ")
        );

        let mut rows = formats.lines();
        let images = rows.next().unwrap().strip_prefix("  画像: ").unwrap();
        let image_names: Vec<_> = images.split(", ").collect();
        assert_eq!(
            image_names,
            crate::image::StandardDecoder::formats()
                .iter()
                .map(|&(name, _)| name)
                .collect::<Vec<_>>()
        );
        assert_eq!(rows.next(), Some("  ドキュメント: PDF"));
        let archives = rows.next().unwrap().strip_prefix("  アーカイブ: ").unwrap();
        let mut formats = archives.strip_suffix(')').unwrap().split("), ");
        for (name, extensions) in crate::archive::builtin_formats() {
            let (actual_name, actual_extensions) =
                formats.next().unwrap().split_once(" (").unwrap();
            assert_eq!(actual_name, name);
            assert_eq!(
                actual_extensions.split(", ").collect::<Vec<_>>(),
                extensions
            );
        }
        assert_eq!(formats.next(), None);
    }
}
