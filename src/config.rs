use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::file_list::SortOrder;
use crate::render::d2d_renderer::AlphaBackground;
use crate::render::layout::DisplayMode;

/// 設定ファイル上の表示モード (DisplayModeへの中間表現)
/// Fixed(f32)はデータ付きバリアントのため、serde直接対応は不可。
/// config側でfixed_scaleと組み合わせてDisplayModeに変換する。
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayModeConfig {
    Shrink,
    #[default]
    Fit,
    Enlarge,
    Original,
    Fixed,
}

/// アプリケーション設定 (ぐらびゅ.toml)
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub display: DisplayConfig,
    pub prefetch: PrefetchConfig,
    pub list: ListConfig,
    pub window: WindowConfig,
    pub susie: SusieConfig,
    pub slideshow: SlideshowConfig,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// 表示モード
    #[serde(deserialize_with = "deserialize_enum_or_default")]
    pub auto_scale: DisplayModeConfig,
    /// 固定倍率 (auto_scale = Fixed のとき使用)
    pub fixed_scale: f32,
    /// 余白量 (ピクセル)
    pub margin: f32,
    /// α背景
    #[serde(deserialize_with = "deserialize_enum_or_default")]
    pub alpha_background: AlphaBackground,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct PrefetchConfig {
    pub cache_base_width: u32,
    pub cache_base_height: u32,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ListConfig {
    /// デフォルトソート
    #[serde(deserialize_with = "deserialize_enum_or_default")]
    pub default_sort: SortOrder,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowConfig {
    pub always_on_top: bool,
    pub keep_titlebar_in_fullscreen: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SusieConfig {
    pub plugin_dir: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SlideshowConfig {
    /// スライドショー間隔 (ミリ秒)
    pub interval_ms: u32,
    /// 最後の画像の後に最初に戻る
    pub repeat: bool,
}

// --- 列挙値のフィールド単位フォールバック ---

trait ConfigEnum: serde::de::DeserializeOwned + Serialize + Default {
    const FIELD: &'static str;
}

impl ConfigEnum for DisplayModeConfig {
    const FIELD: &'static str = "auto_scale";
}

impl ConfigEnum for AlphaBackground {
    const FIELD: &'static str = "alpha_background";
}

impl ConfigEnum for SortOrder {
    const FIELD: &'static str = "default_sort";
}

fn deserialize_enum_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: ConfigEnum,
{
    let value = String::deserialize(deserializer)?;
    let input = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&value);
    if let Ok(parsed) = T::deserialize(input) {
        Ok(parsed)
    } else {
        let default = T::default();
        let encoded = toml::Value::try_from(&default).map_err(serde::de::Error::custom)?;
        eprintln!(
            "警告: {} の値 '{value}' は無効。デフォルト ({}) を使用する。",
            T::FIELD,
            encoded
                .as_str()
                .ok_or_else(|| serde::de::Error::custom("列挙値は文字列で表す"))?
        );
        Ok(default)
    }
}

// --- Default実装 ---

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            auto_scale: DisplayModeConfig::default(),
            fixed_scale: 1.0,
            margin: 64.0,
            alpha_background: AlphaBackground::default(),
        }
    }
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            cache_base_width: 1024,
            cache_base_height: 1536,
        }
    }
}

impl Default for SusieConfig {
    fn default() -> Self {
        Self {
            plugin_dir: "spi".to_string(),
        }
    }
}

impl Default for SlideshowConfig {
    fn default() -> Self {
        Self {
            interval_ms: 3000,
            repeat: true,
        }
    }
}

// --- 変換ヘルパー ---

impl DisplayConfig {
    /// DisplayModeConfigからDisplayModeに変換 (Fixed + fixed_scaleの組み合わせが必要なため維持)
    pub fn to_display_mode(&self) -> DisplayMode {
        match self.auto_scale {
            DisplayModeConfig::Shrink => DisplayMode::AutoShrink,
            DisplayModeConfig::Fit => DisplayMode::AutoFit,
            DisplayModeConfig::Enlarge => DisplayMode::AutoEnlarge,
            DisplayModeConfig::Original => DisplayMode::Original,
            DisplayModeConfig::Fixed => DisplayMode::Fixed(self.fixed_scale),
        }
    }
}

impl PrefetchConfig {
    /// キャッシュ基準サイズ (バイト)。
    ///
    /// `cache_base_width` / `cache_base_height` に 0 が設定されていても、
    /// 後段 (`Document::update_cache_range` 等) でゼロ除算が起きないよう
    /// 最低 1 ピクセルにクランプしてから計算する。
    pub fn base_image_size(&self) -> usize {
        let w = self.cache_base_width.max(1) as usize;
        let h = self.cache_base_height.max(1) as usize;
        w * h * 4
    }
}

impl Config {
    /// exeディレクトリの `ぐらびゅ.toml` を読み込む。
    /// ファイルなし / パース失敗はデフォルトにフォールバック。
    pub fn load() -> Self {
        let Ok(config_path) = crate::paths::config_path() else {
            return Config::default();
        };
        match Self::load_from(&config_path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("警告: 設定ファイルの読み込みに失敗: {e}");
                Config::default()
            }
        }
    }

    /// 指定パスからConfigを読み込む
    pub fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = Config::default();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fit);
        assert_eq!(config.display.fixed_scale, 1.0);
        assert_eq!(config.display.margin, 64.0);
        assert_eq!(config.display.alpha_background, AlphaBackground::Checker);
        assert_eq!(config.prefetch.cache_base_width, 1024);
        assert_eq!(config.prefetch.cache_base_height, 1536);
        assert_eq!(config.list.default_sort, SortOrder::Natural);
        assert!(!config.window.always_on_top);
        assert!(!config.window.keep_titlebar_in_fullscreen);
        assert_eq!(config.susie.plugin_dir, "spi");
    }

    #[test]
    fn parse_full_toml() {
        let toml_str = r#"
[display]
auto_scale = "fit"
fixed_scale = 2.0
margin = 10.0
alpha_background = "black"

[prefetch]
cache_base_width = 800
cache_base_height = 600

[list]
default_sort = "natural"

[window]
always_on_top = true
keep_titlebar_in_fullscreen = true

[susie]
plugin_dir = "plugins"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fit);
        assert_eq!(config.display.fixed_scale, 2.0);
        assert_eq!(config.display.margin, 10.0);
        assert_eq!(config.display.alpha_background, AlphaBackground::Black);
        assert_eq!(config.prefetch.cache_base_width, 800);
        assert_eq!(config.list.default_sort, SortOrder::Natural);
        assert!(config.window.always_on_top);
        assert!(config.window.keep_titlebar_in_fullscreen);
        assert_eq!(config.susie.plugin_dir, "plugins");
    }

    #[test]
    fn parse_partial_toml_fills_defaults() {
        let toml_str = r#"
[display]
auto_scale = "original"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Original);
        // 未指定フィールドはデフォルト
        assert_eq!(config.display.margin, 64.0);
        assert_eq!(config.list.default_sort, SortOrder::Natural);
    }

    #[test]
    fn display_mode_conversion() {
        let d = DisplayConfig {
            auto_scale: DisplayModeConfig::Shrink,
            ..Default::default()
        };
        assert_eq!(d.to_display_mode(), DisplayMode::AutoShrink);

        let d = DisplayConfig {
            auto_scale: DisplayModeConfig::Fit,
            ..Default::default()
        };
        assert_eq!(d.to_display_mode(), DisplayMode::AutoFit);

        let d = DisplayConfig {
            auto_scale: DisplayModeConfig::Fixed,
            fixed_scale: 2.5,
            ..Default::default()
        };
        assert_eq!(d.to_display_mode(), DisplayMode::Fixed(2.5));
    }

    #[test]
    fn invalid_values_fallback_to_defaults() {
        // 無効なauto_scaleはFitにフォールバック
        let toml_str = r#"
[display]
auto_scale = "unknown"
alpha_background = "invalid"

[list]
default_sort = "bogus"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fit);
        assert_eq!(config.display.alpha_background, AlphaBackground::Checker);
        assert_eq!(config.list.default_sort, SortOrder::Natural);
    }

    #[test]
    fn toml_default_matches_rust_default() {
        let distributed: toml::Value =
            toml::from_str(include_str!("../ぐらびゅ.default.toml")).unwrap();
        let defaults = toml::Value::try_from(Config::default()).unwrap();
        assert_eq!(
            distributed, defaults,
            "配布設定のキーと値はConfig::defaultと一致する"
        );
    }

    #[test]
    fn legacy_settings_ignore_removed_fields() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-config.toml");
        let config = Config::load_from(&path).unwrap();
        assert!(config.window.always_on_top);
        assert_eq!(config.susie.plugin_dir, "custom-spi");
        assert_eq!(config.display.margin, 12.0);
    }

    #[test]
    fn invalid_enum_values_warn_without_discarding_valid_fields() {
        let output = std::process::Command::new(crate::paths::exe_path().unwrap())
            .args([
                "--exact",
                "config::tests::invalid_enum_warning_child",
                "--nocapture",
            ])
            .env("GV_CONFIG_WARNING_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        for (field, default) in [
            ("auto_scale", "fit"),
            ("alpha_background", "checker"),
            ("default_sort", "natural"),
        ] {
            assert!(stderr.contains(field), "{stderr}");
            assert!(
                stderr.contains(&format!("デフォルト ({default})")),
                "{stderr}"
            );
        }
    }

    #[test]
    fn invalid_enum_warning_child() {
        if std::env::var_os("GV_CONFIG_WARNING_CHILD").is_none() {
            return;
        }
        let config: Config = toml::from_str("[display]\nauto_scale='invalid'\nalpha_background='invalid'\nmargin=12\n[list]\ndefault_sort='invalid'").unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fit);
        assert_eq!(config.display.alpha_background, AlphaBackground::Checker);
        assert_eq!(config.list.default_sort, SortOrder::Natural);
        assert_eq!(config.display.margin, 12.0);
    }

    #[test]
    fn prefetch_base_image_size() {
        let p = PrefetchConfig::default();
        assert_eq!(p.base_image_size(), 1024 * 1536 * 4);
    }

    #[test]
    fn prefetch_base_image_size_custom() {
        let p = PrefetchConfig {
            cache_base_width: 1920,
            cache_base_height: 1080,
        };
        assert_eq!(p.base_image_size(), 1920 * 1080 * 4);
    }

    #[test]
    fn prefetch_base_image_size_zero_clamps_to_one_pixel() {
        // ゼロ設定は 1×1 ピクセルにクランプされる (document::update_cache_range の
        // ゼロ除算防止)。
        let p = PrefetchConfig {
            cache_base_width: 0,
            cache_base_height: 0,
        };
        assert_eq!(p.base_image_size(), 4);
    }

    #[test]
    fn invalid_auto_scale_fallback() {
        let toml_str = r#"
[display]
auto_scale = "zoom"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fit);
    }

    #[test]
    fn invalid_alpha_background_fallback() {
        let toml_str = r#"
[display]
alpha_background = "red"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.alpha_background, AlphaBackground::Checker);
    }

    #[test]
    fn invalid_sort_order_fallback() {
        let toml_str = r#"
[list]
default_sort = "random"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.list.default_sort, SortOrder::Natural);
    }

    #[test]
    fn all_valid_auto_scale_values() {
        for (value, expected) in [
            ("shrink", DisplayModeConfig::Shrink),
            ("fit", DisplayModeConfig::Fit),
            ("enlarge", DisplayModeConfig::Enlarge),
            ("original", DisplayModeConfig::Original),
            ("fixed", DisplayModeConfig::Fixed),
        ] {
            let toml_str = format!("[display]\nauto_scale = \"{value}\"");
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.display.auto_scale, expected, "auto_scale={value}");
        }
    }

    #[test]
    fn all_valid_alpha_background_values() {
        for (value, expected) in [
            ("white", AlphaBackground::White),
            ("black", AlphaBackground::Black),
            ("checker", AlphaBackground::Checker),
        ] {
            let toml_str = format!("[display]\nalpha_background = \"{value}\"");
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(
                config.display.alpha_background, expected,
                "alpha_background={value}"
            );
        }
    }

    #[test]
    fn all_valid_sort_order_values() {
        for (value, expected) in [
            ("name", SortOrder::Name),
            ("name_nocase", SortOrder::NameNoCase),
            ("size", SortOrder::Size),
            ("date", SortOrder::Date),
            ("natural", SortOrder::Natural),
        ] {
            let toml_str = format!("[list]\ndefault_sort = \"{value}\"");
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.list.default_sort, expected, "default_sort={value}");
        }
    }

    #[test]
    fn display_mode_conversion_enlarge_and_original() {
        let d = DisplayConfig {
            auto_scale: DisplayModeConfig::Enlarge,
            ..Default::default()
        };
        assert_eq!(d.to_display_mode(), DisplayMode::AutoEnlarge);

        let d = DisplayConfig {
            auto_scale: DisplayModeConfig::Original,
            ..Default::default()
        };
        assert_eq!(d.to_display_mode(), DisplayMode::Original);
    }

    #[test]
    fn empty_toml_uses_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        let default = Config::default();
        assert_eq!(config.display.auto_scale, default.display.auto_scale);
        assert_eq!(config.display.fixed_scale, default.display.fixed_scale);
        assert_eq!(config.display.margin, default.display.margin);
        assert_eq!(
            config.prefetch.cache_base_width,
            default.prefetch.cache_base_width
        );
        assert_eq!(
            config.prefetch.cache_base_height,
            default.prefetch.cache_base_height
        );
        assert_eq!(config.list.default_sort, default.list.default_sort);
    }

    #[test]
    fn load_from_nonexistent_file_returns_error() {
        let result = Config::load_from(Path::new("nonexistent_file_xyz.toml"));
        assert!(result.is_err());
    }
}
