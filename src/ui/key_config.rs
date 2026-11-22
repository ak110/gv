use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::action::Action;

const DEFAULT_KEYS_TOML: &str = include_str!("../../ぐらびゅ.keys.default.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    LeftDoubleClick,
    MiddleClick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputChord {
    Key {
        vk: u16,
        modifiers: Modifiers,
    },
    Wheel {
        direction: WheelDirection,
        modifiers: Modifiers,
    },
    Mouse {
        button: MouseButton,
    },
}

pub struct KeyConfig {
    bindings: HashMap<InputChord, Action>,
    chords: HashMap<Action, Vec<InputChord>>,
}

impl KeyConfig {
    pub fn load(path: Option<&Path>) -> Self {
        if let Some(p) = path
            && let Ok(content) = std::fs::read_to_string(p)
        {
            match Self::with_overrides(&content) {
                Ok(config) => return config,
                Err(e) => {
                    eprintln!(
                        "キーバインド設定のパースに失敗しました ({e})。既定のキー割り当てで続行します。\
                         独自の割り当てを使う場合は {} の記述を修正してください。",
                        p.display()
                    );
                }
            }
        }
        Self::with_defaults()
    }

    pub fn with_defaults() -> Self {
        Self::parse_toml(DEFAULT_KEYS_TOML)
            .expect("ぐらびゅ.keys.default.tomlのパースに失敗 (配布物の不整合)")
    }

    pub fn lookup(&self, chord: InputChord) -> Option<Action> {
        self.bindings.get(&chord).copied()
    }

    pub fn chords(&self, action: Action) -> &[InputChord] {
        self.chords.get(&action).map_or(&[], Vec::as_slice)
    }

    pub fn key_labels(&self, action: Action) -> Vec<String> {
        self.chords(action)
            .iter()
            .copied()
            .map(InputChord::display)
            .collect()
    }

    fn with_overrides(content: &str) -> Result<Self> {
        let overrides = Self::parse_toml(content)?;
        let mut config = Self::with_defaults();
        config.chords.retain(|action, chords| {
            if overrides.chords.contains_key(action) {
                return false;
            }
            chords.retain(|chord| !overrides.bindings.contains_key(chord));
            true
        });
        config.chords.extend(overrides.chords);
        config.bindings = config
            .chords
            .iter()
            .flat_map(|(&action, chords)| chords.iter().map(move |&chord| (chord, action)))
            .collect();
        Ok(config)
    }

    fn parse_toml(content: &str) -> Result<Self> {
        let table: toml::Table = content.parse()?;
        let mut bindings = HashMap::new();
        let mut chords: HashMap<Action, Vec<InputChord>> = HashMap::new();

        for (section, value) in &table {
            let Some(section_table) = value.as_table() else {
                continue;
            };
            for (field, val) in section_table {
                let Some(action) = resolve_action(section, field) else {
                    continue;
                };
                let Some(key_str) = val.as_str() else {
                    bail!("[{section}].{field} の割り当ては文字列で指定してください");
                };
                if let Some(old) = chords.insert(action, Vec::new()) {
                    for chord in old {
                        bindings.remove(&chord);
                    }
                }
                for part in key_str.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let chord = parse_chord(part).with_context(|| {
                        format!("[{section}].{field} = {part:?} の入力を解釈できません")
                    })?;
                    if let Some(previous) = bindings.insert(chord, action)
                        && let Some(previous_chords) = chords.get_mut(&previous)
                    {
                        previous_chords.retain(|&c| c != chord);
                    }
                    chords.get_mut(&action).expect("登録済みの操作").push(chord);
                }
            }
        }

        Ok(Self { bindings, chords })
    }
}

impl InputChord {
    pub fn display(self) -> String {
        let (modifiers, name) = match self {
            Self::Mouse { button } => {
                return match button {
                    MouseButton::LeftDoubleClick => "左ダブルクリック",
                    MouseButton::MiddleClick => "ホイールクリック",
                }
                .to_string();
            }
            Self::Wheel {
                direction,
                modifiers,
            } => (
                modifiers,
                match direction {
                    WheelDirection::Up => "ホイール(上)".to_string(),
                    WheelDirection::Down => "ホイール(下)".to_string(),
                },
            ),
            Self::Key { vk, modifiers } => {
                let name = KEY_NAMES.iter().find(|&&(_, code)| code == vk).map_or_else(
                    || {
                        char::from_u32(u32::from(vk))
                            .map_or_else(|| format!("VK {vk:02X}"), |c| c.to_string())
                    },
                    |&(name, _)| name.to_string(),
                );
                (modifiers, name)
            }
        };
        format!(
            "{}{}{}{name}",
            if modifiers.ctrl { "Ctrl+" } else { "" },
            if modifiers.shift { "Shift+" } else { "" },
            if modifiers.alt { "Alt+" } else { "" }
        )
    }
}

fn resolve_action(section: &str, field: &str) -> Option<Action> {
    if section == "persistent_filter" {
        let canonical = if field.starts_with("pfilter_") {
            field.to_string()
        } else {
            format!("pfilter_{field}")
        };
        return field_to_action(&canonical);
    }
    field_to_action(field)
}

pub fn parse_chord(s: &str) -> Result<InputChord> {
    let s = s.trim();

    if let Some(&(_, button)) = MOUSE_NAMES.iter().find(|&&(name, _)| name == s) {
        return Ok(InputChord::Mouse { button });
    }

    let mut modifiers = Modifiers::default();
    let mut remaining = s;

    loop {
        if let Some(rest) = remaining.strip_prefix("Ctrl+") {
            modifiers.ctrl = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Shift+") {
            modifiers.shift = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Alt+") {
            modifiers.alt = true;
            remaining = rest;
        } else {
            break;
        }
    }

    if let Some(&(_, direction)) = WHEEL_NAMES.iter().find(|&&(name, _)| name == remaining) {
        return Ok(InputChord::Wheel {
            direction,
            modifiers,
        });
    }

    let vk = key_name_to_vk(remaining)?;
    Ok(InputChord::Key { vk, modifiers })
}

fn key_name_to_vk(name: &str) -> Result<u16> {
    if name.len() == 1 {
        let ch = name
            .chars()
            .next()
            .expect("name.len() == 1 なら chars() は1要素を返す");
        if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
            return Ok(ch as u16);
        }
    }

    for &(key_name, vk) in KEY_NAMES {
        if key_name.eq_ignore_ascii_case(name) {
            return Ok(vk);
        }
    }

    bail!("不明なキー名: {name:?}");
}

const MOUSE_NAMES: &[(&str, MouseButton)] = &[
    ("LeftDoubleClick", MouseButton::LeftDoubleClick),
    ("左ダブルクリック", MouseButton::LeftDoubleClick),
    ("MiddleClick", MouseButton::MiddleClick),
    ("ホイールクリック", MouseButton::MiddleClick),
];

const WHEEL_NAMES: &[(&str, WheelDirection)] = &[
    ("WheelUp", WheelDirection::Up),
    ("ホイール(上)", WheelDirection::Up),
    ("WheelDown", WheelDirection::Down),
    ("ホイール(下)", WheelDirection::Down),
];

const KEY_NAMES: &[(&str, u16)] = &[
    ("←", 0x25),
    ("→", 0x27),
    ("↑", 0x26),
    ("↓", 0x28),
    ("Left", 0x25),
    ("Right", 0x27),
    ("Up", 0x26),
    ("Down", 0x28),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Home", 0x24),
    ("End", 0x23),
    ("Enter", 0x0D),
    ("Return", 0x0D),
    ("Space", 0x20),
    ("Tab", 0x09),
    ("Esc", 0x1B),
    ("Escape", 0x1B),
    ("BackSpace", 0x08),
    ("Delete", 0x2E),
    ("F1", 0x70),
    ("F2", 0x71),
    ("F3", 0x72),
    ("F4", 0x73),
    ("F5", 0x74),
    ("F6", 0x75),
    ("F7", 0x76),
    ("F8", 0x77),
    ("F9", 0x78),
    ("F10", 0x79),
    ("F11", 0x7A),
    ("F12", 0x7B),
    ("Num0", 0x60),
    ("Num1", 0x61),
    ("Num2", 0x62),
    ("Num3", 0x63),
    ("Num4", 0x64),
    ("Num5", 0x65),
    ("Num6", 0x66),
    ("Num7", 0x67),
    ("Num8", 0x68),
    ("Num9", 0x69),
    ("Num +", 0x6B),
    ("Num -", 0x6D),
    ("Num *", 0x6A),
    ("Num /", 0x6F),
];

fn field_to_action(field: &str) -> Option<Action> {
    Some(match field {
        "back" | "navigate_back" | "page_back" | "navigate_1_back" => Action::NavigateBack,
        "forward" | "navigate_forward" | "page_forward" | "navigate_1_forward" => {
            Action::NavigateForward
        }
        "navigate_5_back" => Action::Navigate5Back,
        "navigate_5_forward" => Action::Navigate5Forward,
        "navigate_50_back" => Action::Navigate50Back,
        "navigate_50_forward" => Action::Navigate50Forward,
        "navigate_first" => Action::NavigateFirst,
        "navigate_last" => Action::NavigateLast,
        "navigate_prev_folder" => Action::NavigatePrevFolder,
        "navigate_next_folder" => Action::NavigateNextFolder,
        "navigate_prev_mark" => Action::NavigatePrevMark,
        "navigate_next_mark" => Action::NavigateNextMark,
        "navigate_to_page" => Action::NavigateToPage,
        "sort_navigate_back" => Action::SortNavigateBack,
        "sort_navigate_forward" => Action::SortNavigateForward,
        "shuffle_all" => Action::ShuffleAll,
        "shuffle_groups" => Action::ShuffleGroups,

        "auto_shrink" => Action::DisplayAutoShrink,
        "auto_fit" => Action::DisplayAutoFit,
        "zoom_in" => Action::ZoomIn,
        "zoom_out" => Action::ZoomOut,
        "zoom_reset" => Action::ZoomReset,
        "toggle_margin" => Action::ToggleMargin,
        "cycle_alpha_background" => Action::CycleAlphaBackground,

        "toggle_fullscreen" => Action::ToggleFullscreen,
        "minimize" => Action::Minimize,
        "toggle_maximize" => Action::ToggleMaximize,
        "toggle_always_on_top" => Action::ToggleAlwaysOnTop,
        "toggle_cursor_hide" => Action::ToggleCursorHide,
        "toggle_menu_bar" => Action::ToggleMenuBar,

        "new_window" => Action::NewWindow,
        "open_file" => Action::OpenFile,
        "open_folder" => Action::OpenFolder,
        "close_all" => Action::CloseAll,
        "reload" => Action::Reload,
        "remove_from_list" => Action::RemoveFromList,
        "delete_file" => Action::DeleteFile,
        "move_file" => Action::MoveFile,
        "copy_file" => Action::CopyFile,
        "open_containing_folder" => Action::OpenContainingFolder,
        "copy_filename" => Action::CopyFileName,
        "copy_image" => Action::CopyImage,
        "paste_image" => Action::PasteImage,
        "export_jpg" => Action::ExportJpg,
        "export_bmp" => Action::ExportBmp,
        "export_png" => Action::ExportPng,
        "show_image_info" => Action::ShowImageInfo,

        "mark_set" | "set" => Action::MarkSet,
        "mark_unset" | "unset" => Action::MarkUnset,
        "mark_invert_all" | "invert_all" => Action::MarkInvertAll,
        "mark_invert_to_here" | "invert_to_here" => Action::MarkInvertToHere,
        "marked_remove_from_list" => Action::MarkedRemoveFromList,
        "marked_delete" => Action::MarkedDelete,
        "marked_move" => Action::MarkedMove,
        "marked_copy" => Action::MarkedCopy,
        "marked_copy_names" => Action::MarkedCopyNames,

        "deselect_selection" => Action::DeselectSelection,
        "crop" => Action::Crop,
        "flip_horizontal" => Action::FlipHorizontal,
        "flip_vertical" => Action::FlipVertical,
        "rotate_180" => Action::Rotate180,
        "rotate_90_cw" => Action::Rotate90CW,
        "rotate_90_ccw" => Action::Rotate90CCW,
        "rotate_arbitrary" => Action::RotateArbitrary,
        "resize" => Action::Resize,

        "fill" => Action::Fill,
        "levels" => Action::Levels,
        "gamma" => Action::Gamma,
        "brightness_contrast" => Action::BrightnessContrast,
        "mosaic" => Action::Mosaic,
        "gaussian_blur" => Action::GaussianBlur,
        "unsharp_mask" => Action::UnsharpMask,
        "invert_colors" => Action::InvertColors,
        "grayscale_simple" => Action::GrayscaleSimple,
        "grayscale_strict" => Action::GrayscaleStrict,
        "apply_alpha" => Action::ApplyAlpha,
        "blur" => Action::Blur,
        "blur_strong" => Action::BlurStrong,
        "sharpen" => Action::Sharpen,
        "sharpen_strong" => Action::SharpenStrong,
        "median_filter" => Action::MedianFilter,

        "pfilter_toggle" => Action::PFilterToggle,
        "pfilter_flip_h" => Action::PFilterFlipH,
        "pfilter_flip_v" => Action::PFilterFlipV,
        "pfilter_rotate_180" => Action::PFilterRotate180,
        "pfilter_rotate_90_cw" => Action::PFilterRotate90CW,
        "pfilter_rotate_90_ccw" => Action::PFilterRotate90CCW,
        "pfilter_levels" => Action::PFilterLevels,
        "pfilter_gamma" => Action::PFilterGamma,
        "pfilter_brightness_contrast" => Action::PFilterBrightnessContrast,
        "pfilter_grayscale_simple" => Action::PFilterGrayscaleSimple,
        "pfilter_grayscale_strict" => Action::PFilterGrayscaleStrict,
        "pfilter_blur" => Action::PFilterBlur,
        "pfilter_blur_strong" => Action::PFilterBlurStrong,
        "pfilter_sharpen" => Action::PFilterSharpen,
        "pfilter_sharpen_strong" => Action::PFilterSharpenStrong,
        "pfilter_gaussian_blur" => Action::PFilterGaussianBlur,
        "pfilter_unsharp_mask" => Action::PFilterUnsharpMask,
        "pfilter_median_filter" => Action::PFilterMedianFilter,
        "pfilter_invert_colors" => Action::PFilterInvertColors,
        "pfilter_apply_alpha" => Action::PFilterApplyAlpha,

        "bookmark_save" => Action::BookmarkSave,
        "bookmark_load" => Action::BookmarkLoad,

        "toggle_file_list" => Action::ToggleFileList,

        "open_exe_folder" => Action::OpenExeFolder,
        "open_bookmark_folder" => Action::OpenBookmarkFolder,
        "open_spi_folder" => Action::OpenSpiFolder,
        "open_temp_folder" => Action::OpenTempFolder,
        "show_help" => Action::ShowHelp,
        "check_update" => Action::CheckUpdate,
        "open_homepage" => Action::OpenHomepage,
        "register_shell" => Action::RegisterShell,
        "unregister_shell" => Action::UnregisterShell,
        "exit" => Action::Exit,

        "slideshow_toggle" => Action::SlideshowToggle,
        "slideshow_faster" => Action::SlideshowFaster,
        "slideshow_slower" => Action::SlideshowSlower,

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn load_overlays_only_the_specified_action_and_falls_back_on_errors() {
        use strum::IntoEnumIterator as _;
        let dir = crate::test_helpers::TempDir::new("keys_overlay");
        let path = dir.join("keys.toml");
        std::fs::write(&path, "[file]\nopen_file = \"Ctrl+Shift+F12\"\n").unwrap();
        let loaded = super::KeyConfig::load(Some(&path));
        let defaults = super::KeyConfig::with_defaults();
        for action in crate::action::Action::iter() {
            if action != crate::action::Action::OpenFile {
                assert_eq!(loaded.chords(action), defaults.chords(action), "{action:?}");
            }
        }
        assert_eq!(
            loaded.key_labels(crate::action::Action::OpenFile),
            ["Ctrl+Shift+F12"]
        );
        std::fs::write(&path, "[file]\nopen_file = \"\"\n").unwrap();
        assert!(
            super::KeyConfig::load(Some(&path))
                .chords(crate::action::Action::OpenFile)
                .is_empty()
        );
        std::fs::write(&path, "invalid = [").unwrap();
        let fallback = super::KeyConfig::load(Some(&path));
        for action in crate::action::Action::iter() {
            assert_eq!(fallback.chords(action), defaults.chords(action));
        }
    }
    use super::*;

    #[test]
    fn accepted_key_names_are_documented() {
        const GUIDE: &str = include_str!("../../docs/guide/customization.md");
        let guide = GUIDE
            .split("### キー名の記法")
            .nth(1)
            .expect("customization.mdに「キー名の記法」の節がある");
        let mut rows = guide.lines().skip_while(|line| !line.starts_with('|'));
        for row in rows
            .by_ref()
            .skip(2)
            .take_while(|line| line.starts_with('|'))
        {
            let notation = row.split('|').nth(1).unwrap();
            let tokens: Vec<_> = notation.split('`').skip(1).step_by(2).collect();
            let examples: Vec<String> = match tokens.as_slice() {
                ["A", "Z"] => ('A'..='Z').map(|key| key.to_string()).collect(),
                ["0", "9"] => ('0'..='9').map(|key| key.to_string()).collect(),
                ["F1", "F12"] => (1..=12).map(|key| format!("F{key}")).collect(),
                ["Num0", "Num9"] => (0..=9).map(|key| format!("Num{key}")).collect(),
                _ => tokens
                    .iter()
                    .map(|key| {
                        if matches!(*key, "Ctrl+" | "Shift+" | "Alt+") {
                            format!("{key}A")
                        } else {
                            key.to_string()
                        }
                    })
                    .collect(),
            };
            for example in examples {
                assert!(parse_chord(&example).is_ok(), "{example}");
            }
        }
        let toml_notation: String = DEFAULT_KEYS_TOML
            .lines()
            .take_while(|line| line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");

        let mut documented: Vec<(String, String)> = Vec::new();
        for &(name, _) in KEY_NAMES {
            let range = if name.starts_with('F') && name[1..].parse::<u8>().is_ok() {
                Some(("F1", "F12"))
            } else if name.starts_with("Num") && name[3..].parse::<u8>().is_ok() {
                Some(("Num0", "Num9"))
            } else {
                None
            };
            match range {
                Some((first, last)) => {
                    documented.push((format!("`{first}`"), first.to_string()));
                    documented.push((format!("`{last}`"), last.to_string()));
                }
                None => documented.push((format!("`{name}`"), name.to_string())),
            }
        }
        for name in MOUSE_NAMES
            .iter()
            .map(|&(name, _)| name)
            .chain(WHEEL_NAMES.iter().map(|&(name, _)| name))
            .chain(["A", "Z", "0", "9"])
        {
            documented.push((format!("`{name}`"), name.to_string()));
        }

        for (guide_token, toml_token) in &documented {
            assert!(
                guide.contains(guide_token.as_str()),
                "customization.mdの記法の表に{guide_token}が無い"
            );
            assert!(
                toml_notation.contains(toml_token.as_str()),
                "ぐらびゅ.keys.default.tomlの記法コメントに{toml_token}が無い"
            );
        }
    }

    #[test]
    fn partial_overrides_preserve_other_actions_and_display_order() {
        use strum::IntoEnumIterator as _;
        let defaults = KeyConfig::with_defaults();
        let config =
            KeyConfig::with_overrides("[file]\nopen_file = \"Ctrl+Shift+F12, MiddleClick\"")
                .unwrap();
        assert_eq!(
            config.key_labels(Action::OpenFile),
            ["Ctrl+Shift+F12", "ホイールクリック"]
        );
        for action in Action::iter() {
            if !matches!(action, Action::OpenFile | Action::ShowImageInfo) {
                assert_eq!(config.chords(action), defaults.chords(action), "{action:?}");
            }
        }
        assert!(config.chords(Action::ShowImageInfo).is_empty());
        assert_eq!(config.lookup(parse_chord("Ctrl+O").unwrap()), None);
        for chord in config.chords(Action::OpenFile) {
            assert_eq!(config.lookup(*chord), Some(Action::OpenFile));
            assert_eq!(parse_chord(&chord.display()).unwrap(), *chord);
        }
        let disabled = KeyConfig::with_overrides("[file]\nopen_file = \"\"").unwrap();
        assert!(disabled.chords(Action::OpenFile).is_empty());
        assert!(KeyConfig::with_overrides("[file]\nopen_file = \"UnknownKey\"").is_err());
    }

    #[test]
    fn parse_simple_key() {
        let chord = parse_chord("←").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x25,
                modifiers: Modifiers::default()
            }
        );
    }

    #[test]
    fn parse_ctrl_modifier() {
        let chord = parse_chord("Ctrl+Home").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x24,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: false,
                    alt: false
                }
            }
        );
    }

    #[test]
    fn parse_ctrl_shift() {
        let chord = parse_chord("Ctrl+Shift+←").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x25,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false
                }
            }
        );
    }

    #[test]
    fn parse_alt_enter() {
        let chord = parse_chord("Alt+Enter").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x0D,
                modifiers: Modifiers {
                    ctrl: false,
                    shift: false,
                    alt: true
                }
            }
        );
    }

    #[test]
    fn parse_function_keys() {
        let chord = parse_chord("F5").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x74,
                modifiers: Modifiers::default()
            }
        );
    }

    #[test]
    fn parse_numpad_keys() {
        let chord = parse_chord("Num0").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x60,
                modifiers: Modifiers::default()
            }
        );

        let chord = parse_chord("Num +").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x6B,
                modifiers: Modifiers::default()
            }
        );
    }

    #[test]
    fn parse_alpha_key() {
        let chord = parse_chord("A").unwrap();
        assert_eq!(
            chord,
            InputChord::Key {
                vk: 0x41,
                modifiers: Modifiers::default()
            }
        );
    }

    #[test]
    fn parse_wheel() {
        let chord = parse_chord("WheelUp").unwrap();
        assert_eq!(
            chord,
            InputChord::Wheel {
                direction: WheelDirection::Up,
                modifiers: Modifiers::default()
            }
        );

        let chord = parse_chord("Ctrl+WheelDown").unwrap();
        assert_eq!(
            chord,
            InputChord::Wheel {
                direction: WheelDirection::Down,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: false,
                    alt: false
                }
            }
        );
    }

    #[test]
    fn parse_mouse() {
        let chord = parse_chord("LeftDoubleClick").unwrap();
        assert_eq!(
            chord,
            InputChord::Mouse {
                button: MouseButton::LeftDoubleClick
            }
        );

        let chord = parse_chord("MiddleClick").unwrap();
        assert_eq!(
            chord,
            InputChord::Mouse {
                button: MouseButton::MiddleClick
            }
        );
    }

    #[test]
    fn parse_unknown_key_returns_error() {
        let error = parse_chord("UnknownKey").unwrap_err().to_string();
        assert!(
            error.contains("不明なキー名") && error.contains("UnknownKey"),
            "{error}"
        );
        assert!(parse_chord("Ctrl+PageUp").is_ok());
    }

    #[test]
    fn default_bindings_complete() {
        let config = KeyConfig::with_defaults();

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x25,
                modifiers: Modifiers::default()
            }),
            Some(Action::NavigateBack)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x27,
                modifiers: Modifiers::default()
            }),
            Some(Action::NavigateForward)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x21,
                modifiers: Modifiers::default()
            }),
            Some(Action::Navigate5Back)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x24,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::NavigateFirst)
        );

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x6F,
                modifiers: Modifiers::default()
            }),
            Some(Action::DisplayAutoShrink)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'A' as u16,
                modifiers: Modifiers::default()
            }),
            Some(Action::CycleAlphaBackground)
        );

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x0D,
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::ToggleFullscreen)
        );

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'O' as u16,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::OpenFile)
        );

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x2E,
                modifiers: Modifiers::default()
            }),
            Some(Action::MarkSet)
        );

        assert_eq!(
            config.lookup(InputChord::Mouse {
                button: MouseButton::LeftDoubleClick
            }),
            Some(Action::ToggleMaximize)
        );
        assert_eq!(
            config.lookup(InputChord::Mouse {
                button: MouseButton::MiddleClick
            }),
            Some(Action::ShowImageInfo)
        );
    }

    #[test]
    fn toml_parse() {
        let toml = r#"
[navigation]
back = "A"
forward = "Ctrl+B"

[display]
zoom_in = "Ctrl+Num +, Ctrl+WheelDown"
"#;
        let config = KeyConfig::parse_toml(toml).unwrap();
        assert_eq!(
            config.lookup(parse_chord("A").unwrap()),
            Some(Action::NavigateBack)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+B").unwrap()),
            Some(Action::NavigateForward)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+Num +").unwrap()),
            Some(Action::ZoomIn)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+WheelDown").unwrap()),
            Some(Action::ZoomIn)
        );
    }

    #[test]
    fn field_to_action_coverage() {
        assert_eq!(field_to_action("back"), Some(Action::NavigateBack));
        assert_eq!(field_to_action("forward"), Some(Action::NavigateForward));
        assert_eq!(
            field_to_action("toggle_fullscreen"),
            Some(Action::ToggleFullscreen)
        );
        assert_eq!(field_to_action("mark_set"), Some(Action::MarkSet));
        assert_eq!(field_to_action("set"), Some(Action::MarkSet));
        assert_eq!(field_to_action("unknown_field"), None);
    }

    #[test]
    fn default_toml_parses_and_resolves() {
        let config = KeyConfig::parse_toml(DEFAULT_KEYS_TOML)
            .expect("配布版ぐらびゅ.keys.default.tomlのパースに失敗");

        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x25,
                modifiers: Modifiers::default()
            }),
            Some(Action::NavigateBack)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x09,
                modifiers: Modifiers::default()
            }),
            Some(Action::SortNavigateForward)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x27,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::Rotate90CW)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'L' as u16,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::Levels)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'G' as u16,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::GrayscaleStrict)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'L' as u16,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::PFilterLevels)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: b'G' as u16,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::PFilterGrayscaleSimple)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x78,
                modifiers: Modifiers::default()
            }),
            Some(Action::BookmarkSave)
        );
        assert_eq!(
            config.lookup(InputChord::Key {
                vk: 0x20,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                }
            }),
            Some(Action::SlideshowToggle)
        );
    }

    #[test]
    fn persistent_filter_section_resolves_to_pfilter_actions() {
        let toml = r#"
[filter]
levels = "Ctrl+L"
grayscale_simple = "Ctrl+Alt+L"

[persistent_filter]
levels = "Ctrl+Shift+L"
grayscale_simple = "Ctrl+Shift+G"
"#;
        let config = KeyConfig::parse_toml(toml).unwrap();
        assert_eq!(
            config.lookup(parse_chord("Ctrl+L").unwrap()),
            Some(Action::Levels)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+Alt+L").unwrap()),
            Some(Action::GrayscaleSimple)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+Shift+L").unwrap()),
            Some(Action::PFilterLevels)
        );
        assert_eq!(
            config.lookup(parse_chord("Ctrl+Shift+G").unwrap()),
            Some(Action::PFilterGrayscaleSimple)
        );
    }

    #[test]
    fn persistent_filter_section_accepts_explicit_prefix() {
        let toml = r#"
[persistent_filter]
pfilter_levels = "Ctrl+Shift+L"
"#;
        let config = KeyConfig::parse_toml(toml).unwrap();
        assert_eq!(
            config.lookup(parse_chord("Ctrl+Shift+L").unwrap()),
            Some(Action::PFilterLevels)
        );
    }
}
