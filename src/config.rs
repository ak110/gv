use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::file_list::SortOrder;
use crate::render::d2d_renderer::AlphaBackground;
use crate::render::layout::DisplayMode;

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
    #[serde(deserialize_with = "deserialize_enum_or_default")]
    pub auto_scale: DisplayModeConfig,
    pub fixed_scale: f32,
    pub margin: f32,
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
    pub interval_ms: u32,
    pub repeat: bool,
}

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

impl DisplayConfig {
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
    pub fn base_image_size(&self) -> usize {
        let w = self.cache_base_width.max(1) as usize;
        let h = self.cache_base_height.max(1) as usize;
        w * h * 4
    }
}

impl Config {
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
    fn parse_full_toml() {
        let toml_str = r#"
[display]
auto_scale = "fixed"
fixed_scale = 2.0
margin = 10.0
alpha_background = "black"

[prefetch]
cache_base_width = 800
cache_base_height = 600

[list]
default_sort = "size"

[window]
always_on_top = true
keep_titlebar_in_fullscreen = true

[susie]
plugin_dir = "plugins"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.display.auto_scale, DisplayModeConfig::Fixed);
        assert_eq!(config.display.fixed_scale, 2.0);
        assert_eq!(config.display.margin, 10.0);
        assert_eq!(config.display.alpha_background, AlphaBackground::Black);
        assert_eq!(config.prefetch.cache_base_width, 800);
        assert_eq!(config.prefetch.cache_base_height, 600);
        assert_eq!(config.list.default_sort, SortOrder::Size);
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
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("test config::tests::invalid_enum_warning_child ... ok"),
            "{stdout}"
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        for (field, default) in [
            ("auto_scale", "fit"),
            ("alpha_background", "checker"),
            ("default_sort", "natural"),
        ] {
            assert!(
                stderr.lines().any(|line| line.contains(field)
                    && line.contains("'invalid'")
                    && line.contains(&format!("デフォルト ({default})"))),
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
        let config: Config = toml::from_str(include_str!("../ぐらびゅ.default.toml")).unwrap();
        assert_eq!(config.prefetch.base_image_size(), 6_291_456);
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
        let p = PrefetchConfig {
            cache_base_width: 0,
            cache_base_height: 0,
        };
        assert_eq!(p.base_image_size(), 4);
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
        let distributed: toml::Value =
            toml::from_str(include_str!("../ぐらびゅ.default.toml")).unwrap();
        assert_eq!(toml::Value::try_from(config).unwrap(), distributed);
    }

    #[test]
    fn load_from_nonexistent_file_returns_error() {
        let dir = crate::test_helpers::TempDir::new("missing_config");
        let error = Config::load_from(&dir.join("missing.toml")).unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
