/// 表示モード
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum DisplayMode {
    /// ウィンドウに収まるよう縮小（拡大はしない）
    AutoShrink,
    /// ウィンドウに合わせて拡大・縮小
    AutoFit,
    /// 原寸大表示
    Original,
    /// 固定倍率
    Fixed(f32),
}

/// 描画先矩形
#[derive(Debug, Clone, Copy)]
pub struct DrawRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct Layout {
    pub mode: DisplayMode,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            mode: DisplayMode::AutoShrink,
        }
    }

    /// 画像サイズとウィンドウサイズから描画先矩形を計算
    pub fn calculate(
        &self,
        image_width: u32,
        image_height: u32,
        window_width: f32,
        window_height: f32,
    ) -> DrawRect {
        let img_w = image_width as f32;
        let img_h = image_height as f32;

        let scale = match self.mode {
            DisplayMode::AutoShrink => {
                let scale_x = window_width / img_w;
                let scale_y = window_height / img_h;
                // 縮小のみ（1.0を超えない）
                scale_x.min(scale_y).min(1.0)
            }
            DisplayMode::AutoFit => {
                let scale_x = window_width / img_w;
                let scale_y = window_height / img_h;
                scale_x.min(scale_y)
            }
            DisplayMode::Original => 1.0,
            DisplayMode::Fixed(s) => s,
        };

        let draw_w = img_w * scale;
        let draw_h = img_h * scale;

        // ウィンドウ中央に配置
        DrawRect {
            x: (window_width - draw_w) / 2.0,
            y: (window_height - draw_h) / 2.0,
            width: draw_w,
            height: draw_h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_shrink_scales_down_large_image() {
        let layout = Layout::new();
        let rect = layout.calculate(2000, 1000, 800.0, 600.0);
        // アスペクト比維持で縮小
        assert!(rect.width <= 800.0);
        assert!(rect.height <= 600.0);
        // 中央配置
        assert!(rect.x >= 0.0);
        assert!(rect.y >= 0.0);
    }

    #[test]
    fn auto_shrink_does_not_enlarge_small_image() {
        let layout = Layout::new();
        let rect = layout.calculate(100, 100, 800.0, 600.0);
        // 原寸のまま
        assert!((rect.width - 100.0).abs() < 0.01);
        assert!((rect.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn auto_fit_enlarges_small_image() {
        let layout = Layout {
            mode: DisplayMode::AutoFit,
        };
        let rect = layout.calculate(100, 100, 800.0, 600.0);
        // ウィンドウに合わせて拡大
        assert!((rect.width - 600.0).abs() < 0.01);
        assert!((rect.height - 600.0).abs() < 0.01);
    }

    #[test]
    fn original_mode_keeps_1x() {
        let layout = Layout {
            mode: DisplayMode::Original,
        };
        let rect = layout.calculate(400, 300, 800.0, 600.0);
        assert!((rect.width - 400.0).abs() < 0.01);
        assert!((rect.height - 300.0).abs() < 0.01);
    }

    #[test]
    fn fixed_mode_applies_scale() {
        let layout = Layout {
            mode: DisplayMode::Fixed(2.0),
        };
        let rect = layout.calculate(100, 100, 800.0, 600.0);
        assert!((rect.width - 200.0).abs() < 0.01);
        assert!((rect.height - 200.0).abs() < 0.01);
    }
}
