//! メニューバー構築
//!
//! Win32 HMENU を構築し、メニューIDからActionへの変換を提供する。

use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::Action;
use crate::ui::key_config::KeyConfig;

/// メニューID の基底値
const WM_COMMAND_BASE: u16 = 0x1000;

/// メニューバーを構築して返す
pub fn build_menu_bar(keys: &KeyConfig) -> HMENU {
    unsafe {
        let menu_bar = CreateMenu().unwrap_or_default();

        // ファイル(&F)
        let file_menu = create_popup(
            keys,
            &[
                Some((Action::NewWindow, "新規ウィンドウ(&N)")),
                Some((Action::OpenFile, "ファイルを開く(&O)")),
                Some((Action::OpenFolder, "フォルダを開く(&F)")),
                None,
                Some((Action::CopyFile, "ファイルを複製(&S)")),
                Some((Action::MoveFile, "ファイルを移動(&R)")),
                Some((Action::DeleteFile, "ファイルを削除")),
                Some((Action::RemoveFromList, "リストから削除")),
                None,
                Some((Action::Reload, "再読み込み(&L)")),
                Some((Action::CloseAll, "全て閉じる(&W)")),
                None,
                Some((Action::Exit, "終了(&X)")),
            ],
        );
        append_popup(menu_bar, file_menu, "ファイル(&F)");

        // マーク(&M)
        let mark_menu = create_popup(
            keys,
            &[
                Some((Action::MarkSet, "マークを設定")),
                Some((Action::MarkUnset, "マークを解除")),
                Some((Action::MarkInvertAll, "全てのマークを反転")),
                Some((Action::MarkInvertToHere, "ここまでのマークを反転")),
                None,
                Some((Action::NavigatePrevMark, "前のマーク画像へ")),
                Some((Action::NavigateNextMark, "次のマーク画像へ")),
                None,
                Some((Action::MarkedRemoveFromList, "マークをリストから削除")),
                Some((Action::MarkedDelete, "マークを完全に削除")),
                Some((Action::MarkedMove, "マークを移動")),
                Some((Action::MarkedCopy, "マークを複製")),
                Some((Action::MarkedCopyNames, "マークのファイル名をコピー")),
            ],
        );
        append_popup(menu_bar, mark_menu, "マーク(&M)");

        // 編集(&E)
        let edit_menu = create_popup(
            keys,
            &[
                Some((Action::FlipHorizontal, "左右反転")),
                Some((Action::FlipVertical, "上下反転")),
                None,
                Some((Action::Rotate180, "180度回転")),
                Some((Action::Rotate90CW, "時計回りに90度回転")),
                Some((Action::Rotate90CCW, "反時計回りに90度回転")),
                Some((Action::RotateArbitrary, "角度指定回転")),
                None,
                Some((Action::Resize, "解像度の変更")),
            ],
        );
        append_popup(menu_bar, edit_menu, "編集(&E)");

        // 画像(&I)
        let image_menu = create_popup(
            keys,
            &[
                Some((Action::DeselectSelection, "選択範囲を取り消し")),
                Some((Action::Crop, "画像のトリミング")),
                None,
                Some((Action::Fill, "塗り潰す")),
                Some((Action::Levels, "レベル補正")),
                Some((Action::Gamma, "ガンマ補正")),
                Some((Action::BrightnessContrast, "明るさとコントラスト")),
                Some((Action::Mosaic, "モザイク")),
                None,
                Some((Action::Blur, "ぼかし")),
                Some((Action::BlurStrong, "ぼかし (強)")),
                Some((Action::Sharpen, "シャープ")),
                Some((Action::SharpenStrong, "シャープ (強)")),
                Some((Action::GaussianBlur, "ガウスぼかし")),
                Some((Action::UnsharpMask, "アンシャープマスク")),
                Some((Action::MedianFilter, "メディアンフィルタ")),
                None,
                Some((Action::InvertColors, "色を反転")),
                Some((Action::GrayscaleSimple, "簡易グレースケール化")),
                Some((Action::GrayscaleStrict, "厳密グレースケール化")),
                Some((Action::ApplyAlpha, "αチャンネルの反映")),
                None,
                Some((Action::CopyImage, "画像をコピー")),
                Some((Action::PasteImage, "クリップボードから貼り付け")),
                Some((Action::CopyFileName, "ファイル名をコピー")),
                None,
                Some((Action::ExportJpg, "JPGとして保存")),
                Some((Action::ExportBmp, "BMPとして保存")),
                Some((Action::ExportPng, "PNGとして保存")),
                None,
                Some((Action::ShowImageInfo, "画像情報")),
            ],
        );
        append_popup(menu_bar, image_menu, "画像(&I)");

        // フィルタ(&T)
        let filter_menu = create_popup(
            keys,
            &[
                Some((Action::PFilterToggle, "フィルタを有効にする")),
                None,
                Some((Action::PFilterFlipH, "左右反転")),
                Some((Action::PFilterFlipV, "上下反転")),
                Some((Action::PFilterRotate180, "180度回転")),
                Some((Action::PFilterRotate90CW, "時計回りに90度回転")),
                Some((Action::PFilterRotate90CCW, "反時計回りに90度回転")),
                None,
                Some((Action::PFilterLevels, "レベル補正")),
                Some((Action::PFilterGamma, "ガンマ補正")),
                Some((Action::PFilterBrightnessContrast, "明るさとコントラスト")),
                Some((Action::PFilterGrayscaleSimple, "簡易グレースケール化")),
                Some((Action::PFilterGrayscaleStrict, "厳密グレースケール化")),
                None,
                Some((Action::PFilterBlur, "ぼかし")),
                Some((Action::PFilterBlurStrong, "ぼかし (強)")),
                Some((Action::PFilterSharpen, "シャープ")),
                Some((Action::PFilterSharpenStrong, "シャープ (強)")),
                Some((Action::PFilterGaussianBlur, "ガウスぼかし")),
                Some((Action::PFilterUnsharpMask, "アンシャープマスク")),
                Some((Action::PFilterMedianFilter, "メディアンフィルタ")),
                None,
                Some((Action::PFilterInvertColors, "色の反転")),
                Some((Action::PFilterApplyAlpha, "αチャンネルの反映")),
            ],
        );
        append_popup(menu_bar, filter_menu, "フィルタ(&T)");

        // リスト(&L)
        let list_menu = create_popup(
            keys,
            &[
                Some((Action::NavigateFirst, "最初へ")),
                Some((Action::NavigateLast, "最後へ")),
                Some((Action::NavigateToPage, "ページ指定")),
                None,
                Some((Action::NavigatePrevFolder, "前のフォルダ")),
                Some((Action::NavigateNextFolder, "次のフォルダ")),
                Some((Action::SortNavigateBack, "ソート順で前へ")),
                Some((Action::SortNavigateForward, "ソート順で次へ")),
                None,
                Some((Action::ShuffleAll, "全体をシャッフル")),
                Some((Action::ShuffleGroups, "グループ順をシャッフル")),
                None,
                Some((Action::ToggleFileList, "ファイルリスト")),
                None,
                Some((Action::BookmarkSave, "ブックマーク保存")),
                Some((Action::BookmarkLoad, "ブックマーク読み込み")),
            ],
        );
        append_popup(menu_bar, list_menu, "リスト(&L)");

        // 表示(&V)
        let view_menu = create_popup(
            keys,
            &[
                Some((Action::DisplayAutoShrink, "自動縮小表示")),
                Some((Action::DisplayAutoFit, "自動縮小・拡大表示")),
                None,
                Some((Action::ZoomIn, "拡大")),
                Some((Action::ZoomOut, "縮小")),
                Some((Action::ZoomReset, "等倍")),
                None,
                Some((Action::ToggleMargin, "余白")),
                Some((Action::CycleAlphaBackground, "α背景切替")),
                None,
                Some((Action::ToggleFullscreen, "全画面表示")),
                Some((Action::ToggleAlwaysOnTop, "常に手前に表示")),
                Some((Action::ToggleCursorHide, "カーソル自動非表示")),
                None,
                Some((Action::SlideshowToggle, "スライドショー")),
                Some((Action::SlideshowFaster, "スライドショー加速")),
                Some((Action::SlideshowSlower, "スライドショー減速")),
            ],
        );
        append_popup(menu_bar, view_menu, "表示(&V)");

        // ヘルプ(&H)
        let help_menu = create_popup(
            keys,
            &[
                Some((Action::ShowHelp, "ヘルプ")),
                Some((Action::CheckUpdate, "アップデートを確認...")),
                Some((Action::OpenHomepage, "ホームページを開く...")),
                None,
                Some((Action::RegisterShell, "シェル統合を登録...")),
                Some((Action::UnregisterShell, "シェル統合を解除...")),
                None,
                Some((Action::OpenExeFolder, "実行ファイルのフォルダ")),
                Some((Action::OpenBookmarkFolder, "ブックマークのフォルダ")),
                Some((Action::OpenSpiFolder, "SPIのフォルダ")),
                Some((Action::OpenTempFolder, "一時フォルダ")),
                None,
                Some((Action::OpenContainingFolder, "画像のフォルダを開く")),
            ],
        );
        append_popup(menu_bar, help_menu, "ヘルプ(&H)");

        menu_bar
    }
}

/// Action からメニューIDを計算
pub fn action_to_menu_id(action: Action) -> u16 {
    use strum::IntoEnumIterator;
    let index = Action::iter().position(|a| a == action).unwrap_or(0) as u16;
    WM_COMMAND_BASE + index
}

fn menu_label(keys: &KeyConfig, action: Action, label: &str) -> String {
    match keys.chords(action).first() {
        Some(chord) => format!("{label}\t{}", chord.display()),
        None => label.to_string(),
    }
}

/// メニュー項目の有効/無効を更新
pub fn update_menu_enabled(menu: HMENU, action: Action, enabled: bool) {
    unsafe {
        let flag = if enabled { MF_ENABLED } else { MF_GRAYED };
        let _ = EnableMenuItem(menu, action_to_menu_id(action) as u32, MF_BYCOMMAND | flag);
    }
}

/// メニュー項目のチェック状態を更新
pub fn update_menu_check(menu: HMENU, action: Action, checked: bool) {
    unsafe {
        let flag = if checked { MF_CHECKED } else { MF_UNCHECKED };
        let _ = CheckMenuItem(
            menu,
            action_to_menu_id(action) as u32,
            (MF_BYCOMMAND | flag).0,
        );
    }
}

/// メニューIDからActionに変換
pub fn menu_id_to_action(id: u16) -> Option<Action> {
    if id < WM_COMMAND_BASE {
        return None;
    }
    let index = id - WM_COMMAND_BASE;
    action_from_index(index)
}

/// ポップアップメニューを作成してアイテムを追加
unsafe fn create_popup(keys: &KeyConfig, items: &[Option<(Action, &str)>]) -> HMENU {
    unsafe {
        let popup = CreatePopupMenu().unwrap_or_default();
        for item in items {
            match item {
                Some((action, label)) => {
                    let wide_label = to_wide(&menu_label(keys, *action, label));
                    let _ = AppendMenuW(
                        popup,
                        MF_STRING,
                        action_to_menu_id(*action) as usize,
                        windows::core::PCWSTR(wide_label.as_ptr()),
                    );
                }
                None => {
                    let _ = AppendMenuW(popup, MF_SEPARATOR, 0, None);
                }
            }
        }
        popup
    }
}

/// ポップアップをメニューバーに追加
unsafe fn append_popup(menu_bar: HMENU, popup: HMENU, label: &str) {
    unsafe {
        let wide_label = to_wide(label);
        let _ = AppendMenuW(
            menu_bar,
            MF_POPUP,
            popup.0 as usize,
            windows::core::PCWSTR(wide_label.as_ptr()),
        );
    }
}

use crate::util::to_wide;

/// インデックスからActionを復元する (strum::EnumIter を利用)
fn action_from_index(index: u16) -> Option<Action> {
    use strum::IntoEnumIterator;
    Action::iter().nth(index as usize)
}

#[cfg(test)]
mod tests {
    #[test]
    fn custom_menu_label_uses_the_first_key_and_omits_cleared_bindings() {
        let dir = crate::test_helpers::TempDir::new("menu_keys");
        let path = dir.join("keys.toml");
        std::fs::write(
            &path,
            "[file]\nopen_file = \"Ctrl+Shift+F12, MiddleClick\"\nopen_folder = \"\"\n",
        )
        .unwrap();
        let keys = super::KeyConfig::load(Some(&path));
        assert_eq!(
            super::menu_label(&keys, crate::action::Action::OpenFile, "開く"),
            "開く\tCtrl+Shift+F12"
        );
        assert_eq!(
            super::menu_label(&keys, crate::action::Action::OpenFolder, "フォルダ"),
            "フォルダ"
        );
    }
    use super::*;
    use strum::IntoEnumIterator as _;

    #[test]
    fn menu_labels_follow_the_first_binding() {
        let keys = KeyConfig::with_defaults();
        for action in Action::iter() {
            let label = menu_label(&keys, action, "操作");
            let accelerator = label.split_once('\t').map(|(_, key)| key);
            let first = keys.chords(action).first().map(|key| key.display());
            assert_eq!(accelerator, first.as_deref(), "{action:?}");
            assert_eq!(menu_id_to_action(action_to_menu_id(action)), Some(action));
        }
    }
}
