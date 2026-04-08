//! 矩形選択の状態管理
//!
//! 画像ピクセル座標ベースの状態機械。マウスドラッグで矩形選択し、
//! リサイズハンドルやドラッグ移動で選択範囲を編集できる。

use crate::render::layout::DrawRect;

/// 画像ピクセル座標の矩形 (正規化済み: width/height >= 0)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    /// 2点から正規化された矩形を生成 (左上・右下に正規化)
    pub fn from_two_points(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        let (lx, rx) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
        let (ty, by) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
        Self {
            x: lx,
            y: ty,
            width: rx - lx,
            height: by - ty,
        }
    }

    /// 画像範囲にクランプする
    pub fn clamped(&self, img_width: u32, img_height: u32) -> Self {
        let iw = img_width as i32;
        let ih = img_height as i32;
        let x = self.x.clamp(0, iw);
        let y = self.y.clamp(0, ih);
        let r = (self.x + self.width).clamp(0, iw);
        let b = (self.y + self.height).clamp(0, ih);
        Self {
            x,
            y,
            width: r - x,
            height: b - y,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// 矩形が有効 (面積 > 0) か
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// 8方向リサイズハンドル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// 選択の状態遷移
#[derive(Debug, Clone)]
pub enum SelectionState {
    /// 選択なし
    None,
    /// ドラッグ中 (新規選択)
    Drawing {
        start_px: i32,
        start_py: i32,
        current_px: i32,
        current_py: i32,
    },
    /// 選択確定
    Selected { rect: PixelRect },
    /// リサイズ中
    Resizing {
        rect: PixelRect,
        handle: HandleKind,
        /// ドラッグ開始時の画像ピクセル座標
        start_px: i32,
        start_py: i32,
    },
    /// 移動中
    Moving {
        rect: PixelRect,
        /// ドラッグ開始時の画像ピクセル座標
        start_px: i32,
        start_py: i32,
    },
}

/// 矩形選択マネージャ
pub struct Selection {
    state: SelectionState,
}

/// ハンドルのヒット判定半径 (スクリーンピクセル)
const HANDLE_HIT_RADIUS: f32 = 6.0;
/// ハンドル描画サイズ (スクリーンピクセル)
pub const HANDLE_DRAW_SIZE: f32 = 5.0;

impl Selection {
    pub fn new() -> Self {
        Self {
            state: SelectionState::None,
        }
    }

    #[allow(dead_code)] // 将来のPhase (フィルタ適用時の選択領域判定等) で使用予定
    pub fn state(&self) -> &SelectionState {
        &self.state
    }

    /// 確定済み選択矩形を返す (Drawing中は未確定の矩形を返す)
    pub fn current_rect(&self) -> Option<PixelRect> {
        match &self.state {
            SelectionState::None => None,
            SelectionState::Drawing {
                start_px,
                start_py,
                current_px,
                current_py,
            } => {
                let rect =
                    PixelRect::from_two_points(*start_px, *start_py, *current_px, *current_py);
                if rect.is_valid() { Some(rect) } else { None }
            }
            SelectionState::Selected { rect }
            | SelectionState::Resizing { rect, .. }
            | SelectionState::Moving { rect, .. } => Some(*rect),
        }
    }

    /// 選択が確定状態 (Selected) か
    pub fn is_selected(&self) -> bool {
        matches!(self.state, SelectionState::Selected { .. })
    }

    /// ドラッグ操作中 (Drawing/Resizing/Moving) か
    pub fn is_dragging(&self) -> bool {
        matches!(
            self.state,
            SelectionState::Drawing { .. }
                | SelectionState::Resizing { .. }
                | SelectionState::Moving { .. }
        )
    }

    /// 選択を解除する
    pub fn deselect(&mut self) {
        self.state = SelectionState::None;
    }

    /// マウス押下: 状態遷移を行う
    /// `px`, `py` は画像ピクセル座標。`sx`, `sy` はスクリーン座標。
    /// draw_rect と image_size は座標変換用。
    pub fn on_mouse_down(
        &mut self,
        sx: f32,
        sy: f32,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) {
        let (px, py) = screen_to_image(sx, sy, draw_rect, img_width, img_height);

        // 画像の外ならば何もしない
        if px < 0 || py < 0 || px >= img_width as i32 || py >= img_height as i32 {
            return;
        }

        match &self.state {
            SelectionState::Selected { rect } => {
                // ハンドル上→リサイズ開始
                if let Some(handle) =
                    hit_test_handle(sx, sy, rect, draw_rect, img_width, img_height)
                {
                    self.state = SelectionState::Resizing {
                        rect: *rect,
                        handle,
                        start_px: px,
                        start_py: py,
                    };
                    return;
                }
                // 選択内部→移動開始
                if rect.x <= px && px < rect.right() && rect.y <= py && py < rect.bottom() {
                    self.state = SelectionState::Moving {
                        rect: *rect,
                        start_px: px,
                        start_py: py,
                    };
                    return;
                }
                // 選択外部→新規選択
                self.state = SelectionState::Drawing {
                    start_px: px,
                    start_py: py,
                    current_px: px,
                    current_py: py,
                };
            }
            _ => {
                // None / Drawing → 新規選択開始
                self.state = SelectionState::Drawing {
                    start_px: px,
                    start_py: py,
                    current_px: px,
                    current_py: py,
                };
            }
        }
    }

    /// マウス移動: ドラッグ中の矩形更新
    pub fn on_mouse_move(
        &mut self,
        sx: f32,
        sy: f32,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) {
        let (px, py) = screen_to_image(sx, sy, draw_rect, img_width, img_height);
        // 画像範囲にクランプ
        let px = px.clamp(0, img_width as i32);
        let py = py.clamp(0, img_height as i32);

        match &mut self.state {
            SelectionState::Drawing {
                current_px,
                current_py,
                ..
            } => {
                *current_px = px;
                *current_py = py;
            }
            SelectionState::Resizing {
                rect,
                handle,
                start_px,
                start_py,
            } => {
                let dx = px - *start_px;
                let dy = py - *start_py;
                *rect = apply_resize(*rect, *handle, dx, dy);
                *start_px = px;
                *start_py = py;
            }
            SelectionState::Moving {
                rect,
                start_px,
                start_py,
            } => {
                let dx = px - *start_px;
                let dy = py - *start_py;
                rect.x += dx;
                rect.y += dy;
                *start_px = px;
                *start_py = py;
            }
            _ => {}
        }
    }

    /// マウスリリース: ドラッグ終了
    pub fn on_mouse_up(&mut self, img_width: u32, img_height: u32) {
        match &self.state {
            SelectionState::Drawing {
                start_px,
                start_py,
                current_px,
                current_py,
            } => {
                let rect =
                    PixelRect::from_two_points(*start_px, *start_py, *current_px, *current_py)
                        .clamped(img_width, img_height);
                if rect.is_valid() {
                    self.state = SelectionState::Selected { rect };
                } else {
                    self.state = SelectionState::None;
                }
            }
            SelectionState::Resizing { rect, .. } | SelectionState::Moving { rect, .. } => {
                let rect = rect.clamped(img_width, img_height);
                if rect.is_valid() {
                    self.state = SelectionState::Selected { rect };
                } else {
                    self.state = SelectionState::None;
                }
            }
            _ => {}
        }
    }

    /// 指定スクリーン座標がハンドル上にあるかを返す (カーソル形状の決定用)
    pub fn hit_test_at(
        &self,
        sx: f32,
        sy: f32,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) -> HitTestResult {
        let SelectionState::Selected { rect } = &self.state else {
            return HitTestResult::Outside;
        };

        if let Some(handle) = hit_test_handle(sx, sy, rect, draw_rect, img_width, img_height) {
            return HitTestResult::Handle(handle);
        }

        let (px, py) = screen_to_image(sx, sy, draw_rect, img_width, img_height);
        if rect.x <= px && px < rect.right() && rect.y <= py && py < rect.bottom() {
            HitTestResult::Inside
        } else {
            HitTestResult::Outside
        }
    }
}

/// ヒットテスト結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestResult {
    Outside,
    Inside,
    Handle(HandleKind),
}

// --- 座標変換ユーティリティ ---

/// スクリーン座標 → 画像ピクセル座標
pub fn screen_to_image(
    sx: f32,
    sy: f32,
    draw_rect: &DrawRect,
    img_width: u32,
    img_height: u32,
) -> (i32, i32) {
    if draw_rect.width <= 0.0 || draw_rect.height <= 0.0 {
        return (0, 0);
    }
    let px = ((sx - draw_rect.x) / draw_rect.width * img_width as f32).floor() as i32;
    let py = ((sy - draw_rect.y) / draw_rect.height * img_height as f32).floor() as i32;
    (px, py)
}

/// 画像ピクセル座標 → スクリーン座標
pub fn image_to_screen(
    px: i32,
    py: i32,
    draw_rect: &DrawRect,
    img_width: u32,
    img_height: u32,
) -> (f32, f32) {
    let sx = draw_rect.x + px as f32 / img_width as f32 * draw_rect.width;
    let sy = draw_rect.y + py as f32 / img_height as f32 * draw_rect.height;
    (sx, sy)
}

/// ハンドルのヒットテスト
fn hit_test_handle(
    sx: f32,
    sy: f32,
    rect: &PixelRect,
    draw_rect: &DrawRect,
    img_width: u32,
    img_height: u32,
) -> Option<HandleKind> {
    let handles = handle_positions(rect);
    let r = HANDLE_HIT_RADIUS;

    for (kind, hx, hy) in handles {
        let (screen_x, screen_y) = image_to_screen(hx, hy, draw_rect, img_width, img_height);
        if (sx - screen_x).abs() <= r && (sy - screen_y).abs() <= r {
            return Some(kind);
        }
    }
    None
}

/// 8箇所のハンドル位置を画像ピクセル座標で返す
pub fn handle_positions(rect: &PixelRect) -> [(HandleKind, i32, i32); 8] {
    let mx = rect.x + rect.width / 2;
    let my = rect.y + rect.height / 2;
    let r = rect.right();
    let b = rect.bottom();
    [
        (HandleKind::TopLeft, rect.x, rect.y),
        (HandleKind::Top, mx, rect.y),
        (HandleKind::TopRight, r, rect.y),
        (HandleKind::Left, rect.x, my),
        (HandleKind::Right, r, my),
        (HandleKind::BottomLeft, rect.x, b),
        (HandleKind::Bottom, mx, b),
        (HandleKind::BottomRight, r, b),
    ]
}

/// リサイズ適用
fn apply_resize(rect: PixelRect, handle: HandleKind, dx: i32, dy: i32) -> PixelRect {
    let mut x = rect.x;
    let mut y = rect.y;
    let mut w = rect.width;
    let mut h = rect.height;

    match handle {
        HandleKind::TopLeft => {
            x += dx;
            y += dy;
            w -= dx;
            h -= dy;
        }
        HandleKind::Top => {
            y += dy;
            h -= dy;
        }
        HandleKind::TopRight => {
            w += dx;
            y += dy;
            h -= dy;
        }
        HandleKind::Left => {
            x += dx;
            w -= dx;
        }
        HandleKind::Right => {
            w += dx;
        }
        HandleKind::BottomLeft => {
            x += dx;
            w -= dx;
            h += dy;
        }
        HandleKind::Bottom => {
            h += dy;
        }
        HandleKind::BottomRight => {
            w += dx;
            h += dy;
        }
    }

    // 反転防止: 最小サイズ1
    if w < 1 {
        w = 1;
    }
    if h < 1 {
        h = 1;
    }

    PixelRect {
        x,
        y,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_draw_rect() -> DrawRect {
        // 画像100x100を画面の (10,10)-(210,210) に描画している想定
        DrawRect {
            x: 10.0,
            y: 10.0,
            width: 200.0,
            height: 200.0,
        }
    }

    #[test]
    fn screen_to_image_basic() {
        let dr = test_draw_rect();
        // 画面 (110, 110) = 画像の中心 (50, 50)
        let (px, py) = screen_to_image(110.0, 110.0, &dr, 100, 100);
        assert_eq!(px, 50);
        assert_eq!(py, 50);
    }

    #[test]
    fn image_to_screen_basic() {
        let dr = test_draw_rect();
        let (sx, sy) = image_to_screen(50, 50, &dr, 100, 100);
        assert!((sx - 110.0).abs() < 0.01);
        assert!((sy - 110.0).abs() < 0.01);
    }

    #[test]
    fn pixel_rect_from_two_points_normalizes() {
        let rect = PixelRect::from_two_points(50, 60, 10, 20);
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 20);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 40);
    }

    #[test]
    fn pixel_rect_clamp() {
        let rect = PixelRect {
            x: -10,
            y: -5,
            width: 200,
            height: 150,
        };
        let clamped = rect.clamped(100, 100);
        assert_eq!(clamped.x, 0);
        assert_eq!(clamped.y, 0);
        assert_eq!(clamped.width, 100);
        assert_eq!(clamped.height, 100);
    }

    #[test]
    fn selection_draw_and_release() {
        let dr = test_draw_rect();
        let mut sel = Selection::new();

        // マウスダウンで描画開始
        sel.on_mouse_down(30.0, 30.0, &dr, 100, 100);
        assert!(sel.is_dragging());

        // ドラッグ
        sel.on_mouse_move(130.0, 130.0, &dr, 100, 100);
        assert!(sel.current_rect().is_some());

        // リリースで確定
        sel.on_mouse_up(100, 100);
        assert!(sel.is_selected());
        let rect = sel.current_rect().unwrap();
        assert!(rect.width > 0);
        assert!(rect.height > 0);
    }

    #[test]
    fn selection_deselect() {
        let dr = test_draw_rect();
        let mut sel = Selection::new();
        sel.on_mouse_down(30.0, 30.0, &dr, 100, 100);
        sel.on_mouse_move(130.0, 130.0, &dr, 100, 100);
        sel.on_mouse_up(100, 100);
        assert!(sel.is_selected());

        sel.deselect();
        assert!(!sel.is_selected());
        assert!(sel.current_rect().is_none());
    }

    #[test]
    fn handle_positions_count() {
        let rect = PixelRect {
            x: 10,
            y: 20,
            width: 80,
            height: 60,
        };
        let handles = handle_positions(&rect);
        assert_eq!(handles.len(), 8);
    }

    #[test]
    fn apply_resize_bottom_right() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::BottomRight, 10, 20);
        assert_eq!(resized.width, 60);
        assert_eq!(resized.height, 70);
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 10);
    }

    #[test]
    fn apply_resize_top_left() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::TopLeft, 5, 5);
        assert_eq!(resized.x, 15);
        assert_eq!(resized.y, 15);
        assert_eq!(resized.width, 45);
        assert_eq!(resized.height, 45);
    }

    // --- apply_resize: 全8方向ハンドルのテスト ---

    #[test]
    fn apply_resize_top() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::Top, 0, -5);
        // Topハンドル: yが上に5移動、高さが5増加、x/widthは不変
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 5);
        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 55);
    }

    #[test]
    fn apply_resize_top_right() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::TopRight, 10, -5);
        // TopRight: 幅が10増加、yが5上に移動し高さ5増加
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 5);
        assert_eq!(resized.width, 60);
        assert_eq!(resized.height, 55);
    }

    #[test]
    fn apply_resize_left() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::Left, -5, 0);
        // Left: xが5左に移動、幅が5増加、y/heightは不変
        assert_eq!(resized.x, 5);
        assert_eq!(resized.y, 10);
        assert_eq!(resized.width, 55);
        assert_eq!(resized.height, 50);
    }

    #[test]
    fn apply_resize_right() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::Right, 15, 0);
        // Right: 幅が15増加、x/y/heightは不変
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 10);
        assert_eq!(resized.width, 65);
        assert_eq!(resized.height, 50);
    }

    #[test]
    fn apply_resize_bottom_left() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::BottomLeft, -5, 10);
        // BottomLeft: xが5左に移動、幅が5増加、高さが10増加
        assert_eq!(resized.x, 5);
        assert_eq!(resized.y, 10);
        assert_eq!(resized.width, 55);
        assert_eq!(resized.height, 60);
    }

    #[test]
    fn apply_resize_bottom() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::Bottom, 0, 10);
        // Bottom: 高さが10増加、x/y/widthは不変
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 10);
        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 60);
    }

    // --- 反転防止 (最小サイズ1) のテスト ---

    #[test]
    fn apply_resize_clamps_to_minimum_size() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 5,
            height: 5,
        };
        // 幅・高さがマイナスになるほど大きなdxで縮小を試みる
        let resized = apply_resize(rect, HandleKind::Right, -100, 0);
        assert_eq!(resized.width, 1); // 最小1にクランプ
        assert_eq!(resized.height, 5);

        let resized = apply_resize(rect, HandleKind::Bottom, 0, -100);
        assert_eq!(resized.width, 5);
        assert_eq!(resized.height, 1); // 最小1にクランプ
    }

    #[test]
    fn apply_resize_top_left_clamps_both_axes() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 5,
            height: 5,
        };
        // TopLeftで幅・高さ両方が反転するほどのdx, dy
        let resized = apply_resize(rect, HandleKind::TopLeft, 100, 100);
        assert_eq!(resized.width, 1);
        assert_eq!(resized.height, 1);
    }

    // --- ゼロサイズ矩形の正規化テスト ---

    #[test]
    fn zero_size_rect_from_same_point() {
        // 同一点からの矩形は幅・高さ0
        let rect = PixelRect::from_two_points(50, 50, 50, 50);
        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 0);
        assert!(!rect.is_valid()); // 面積0は無効
    }

    #[test]
    fn zero_width_rect_is_invalid() {
        // 幅だけ0の矩形
        let rect = PixelRect::from_two_points(50, 10, 50, 60);
        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 50);
        assert!(!rect.is_valid());
    }

    #[test]
    fn zero_height_rect_is_invalid() {
        // 高さだけ0の矩形
        let rect = PixelRect::from_two_points(10, 50, 60, 50);
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 0);
        assert!(!rect.is_valid());
    }

    #[test]
    fn zero_size_drawing_releases_to_none() {
        // ゼロサイズのドラッグ操作はmouse_upでNoneに戻る
        let dr = test_draw_rect();
        let mut sel = Selection::new();

        // 同一点でドラッグ
        sel.on_mouse_down(110.0, 110.0, &dr, 100, 100);
        // 移動しない (same point)
        sel.on_mouse_up(100, 100);

        assert!(!sel.is_selected());
        assert!(sel.current_rect().is_none());
    }

    #[test]
    fn screen_to_image_zero_size_draw_rect() {
        // draw_rectのwidth/heightが0の場合は (0,0) を返す
        let dr = DrawRect {
            x: 10.0,
            y: 10.0,
            width: 0.0,
            height: 0.0,
        };
        let (px, py) = screen_to_image(50.0, 50.0, &dr, 100, 100);
        assert_eq!(px, 0);
        assert_eq!(py, 0);
    }

    #[test]
    fn hit_test_inside_and_outside() {
        let dr = test_draw_rect();
        let mut sel = Selection::new();

        // 選択範囲を作成: 画像の (20,20)-(80,80)
        sel.on_mouse_down(50.0, 50.0, &dr, 100, 100);
        sel.on_mouse_move(170.0, 170.0, &dr, 100, 100);
        sel.on_mouse_up(100, 100);

        // 選択内部
        let result = sel.hit_test_at(110.0, 110.0, &dr, 100, 100);
        assert_eq!(result, HitTestResult::Inside);

        // 選択外部
        let result = sel.hit_test_at(15.0, 15.0, &dr, 100, 100);
        assert_eq!(result, HitTestResult::Outside);
    }
}
