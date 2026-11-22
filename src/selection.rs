use crate::render::layout::DrawRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
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

    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

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

#[derive(Debug, Clone)]
pub enum SelectionState {
    None,
    Drawing {
        start_px: i32,
        start_py: i32,
        current_px: i32,
        current_py: i32,
    },
    Selected {
        rect: PixelRect,
    },
    Resizing {
        rect: PixelRect,
        handle: HandleKind,
        start_px: i32,
        start_py: i32,
    },
    Moving {
        rect: PixelRect,
        start_px: i32,
        start_py: i32,
    },
}

pub struct Selection {
    state: SelectionState,
}

const HANDLE_HIT_RADIUS: f32 = 6.0;
pub const HANDLE_DRAW_SIZE: f32 = 5.0;

impl Selection {
    pub fn new() -> Self {
        Self {
            state: SelectionState::None,
        }
    }

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

    pub fn is_selected(&self) -> bool {
        matches!(self.state, SelectionState::Selected { .. })
    }

    pub fn is_dragging(&self) -> bool {
        matches!(
            self.state,
            SelectionState::Drawing { .. }
                | SelectionState::Resizing { .. }
                | SelectionState::Moving { .. }
        )
    }

    pub fn deselect(&mut self) {
        self.state = SelectionState::None;
    }

    pub fn on_mouse_down(
        &mut self,
        sx: f32,
        sy: f32,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) {
        let (px, py) = screen_to_image(sx, sy, draw_rect, img_width, img_height);

        if px < 0 || py < 0 || px >= img_width as i32 || py >= img_height as i32 {
            return;
        }

        match &self.state {
            SelectionState::Selected { rect } => {
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
                if rect.x <= px && px < rect.right() && rect.y <= py && py < rect.bottom() {
                    self.state = SelectionState::Moving {
                        rect: *rect,
                        start_px: px,
                        start_py: py,
                    };
                    return;
                }
                self.state = SelectionState::Drawing {
                    start_px: px,
                    start_py: py,
                    current_px: px,
                    current_py: py,
                };
            }
            _ => {
                self.state = SelectionState::Drawing {
                    start_px: px,
                    start_py: py,
                    current_px: px,
                    current_py: py,
                };
            }
        }
    }

    pub fn on_mouse_move(
        &mut self,
        sx: f32,
        sy: f32,
        draw_rect: &DrawRect,
        img_width: u32,
        img_height: u32,
    ) {
        let (px, py) = screen_to_image(sx, sy, draw_rect, img_width, img_height);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestResult {
    Outside,
    Inside,
    Handle(HandleKind),
}

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
    use crate::test_helpers::test_draw_rect;

    #[test]
    fn screen_to_image_basic() {
        let dr = test_draw_rect();
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

        sel.on_mouse_down(30.0, 30.0, &dr, 100, 100);
        assert!(sel.is_dragging());

        sel.on_mouse_move(130.0, 110.0, &dr, 100, 100);
        let expected = Some(PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 40,
        });
        assert_eq!(sel.current_rect(), expected);

        sel.on_mouse_up(100, 100);
        assert!(sel.is_selected());
        assert_eq!(sel.current_rect(), expected);
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

    #[test]
    fn apply_resize_top() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 50,
            height: 50,
        };
        let resized = apply_resize(rect, HandleKind::Top, 0, -5);
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
        assert_eq!(resized.x, 10);
        assert_eq!(resized.y, 10);
        assert_eq!(resized.width, 50);
        assert_eq!(resized.height, 60);
    }

    #[test]
    fn apply_resize_clamps_to_minimum_size() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 5,
            height: 5,
        };
        let resized = apply_resize(rect, HandleKind::Right, -100, 0);
        assert_eq!(resized.width, 1);
        assert_eq!(resized.height, 5);

        let resized = apply_resize(rect, HandleKind::Bottom, 0, -100);
        assert_eq!(resized.width, 5);
        assert_eq!(resized.height, 1);
    }

    #[test]
    fn apply_resize_top_left_clamps_both_axes() {
        let rect = PixelRect {
            x: 10,
            y: 10,
            width: 5,
            height: 5,
        };
        let resized = apply_resize(rect, HandleKind::TopLeft, 100, 100);
        assert_eq!(resized.width, 1);
        assert_eq!(resized.height, 1);
    }

    #[test]
    fn zero_size_rect_from_same_point() {
        let rect = PixelRect::from_two_points(50, 50, 50, 50);
        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 0);
        assert!(!rect.is_valid());
    }

    #[test]
    fn zero_width_rect_is_invalid() {
        let rect = PixelRect::from_two_points(50, 10, 50, 60);
        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 50);
        assert!(!rect.is_valid());
    }

    #[test]
    fn zero_height_rect_is_invalid() {
        let rect = PixelRect::from_two_points(10, 50, 60, 50);
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 0);
        assert!(!rect.is_valid());
    }

    #[test]
    fn zero_size_drawing_releases_to_none() {
        let dr = test_draw_rect();
        let mut sel = Selection::new();

        sel.on_mouse_down(110.0, 110.0, &dr, 100, 100);
        sel.on_mouse_up(100, 100);

        assert!(!sel.is_selected());
        assert!(sel.current_rect().is_none());
    }

    #[test]
    fn screen_to_image_zero_size_draw_rect() {
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

        sel.on_mouse_down(50.0, 50.0, &dr, 100, 100);
        sel.on_mouse_move(170.0, 170.0, &dr, 100, 100);
        sel.on_mouse_up(100, 100);

        let result = sel.hit_test_at(110.0, 110.0, &dr, 100, 100);
        assert_eq!(result, HitTestResult::Inside);

        let result = sel.hit_test_at(15.0, 15.0, &dr, 100, 100);
        assert_eq!(result, HitTestResult::Outside);
    }
}
