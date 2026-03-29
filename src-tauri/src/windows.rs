use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

pub const OBJ_SCALE_W: f64 = 1.9;
pub const OBJ_SCALE_H: f64 = 1.3;
pub const OBJ_OFF_X: f64 = -0.45;
pub const OBJ_OFF_Y: f64 = -0.25;

#[derive(Debug, Clone, Copy)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct HitBox {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct HitRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl HitBox {
    pub const DEFAULT:  HitBox = HitBox { x: -1, y: 5,  w: 17, h: 12 };
    #[allow(dead_code)]
    pub const SLEEPING: HitBox = HitBox { x: -2, y: 9,  w: 19, h: 7  };
    #[allow(dead_code)]
    pub const WIDE:     HitBox = HitBox { x: -3, y: 3,  w: 21, h: 14 };
}

pub fn compute_hit_rect(bounds: &WindowBounds, hb: &HitBox) -> HitRect {
    let obj_x = bounds.x as f64 + bounds.width  as f64 * OBJ_OFF_X;
    let obj_y = bounds.y as f64 + bounds.height as f64 * OBJ_OFF_Y;
    let obj_w = bounds.width  as f64 * OBJ_SCALE_W;
    let obj_h = bounds.height as f64 * OBJ_SCALE_H;

    let scale = obj_w.min(obj_h) / 45.0;
    let offset_x = obj_x + (obj_w - 45.0 * scale) / 2.0;
    let offset_y = obj_y + (obj_h - 45.0 * scale) / 2.0;

    HitRect {
        left:   offset_x + (hb.x as f64 + 15.0) * scale,
        top:    offset_y + (hb.y as f64 + 25.0) * scale,
        right:  offset_x + (hb.x as f64 + 15.0 + hb.w as f64) * scale,
        bottom: offset_y + (hb.y as f64 + 25.0 + hb.h as f64) * scale,
    }
}

pub fn sync_hit_window(app: &AppHandle, pet_bounds: &WindowBounds, hb: &HitBox) {
    let hit_win = match app.get_webview_window("hit") {
        Some(w) => w,
        None => return,
    };
    let rect = compute_hit_rect(pet_bounds, hb);
    let mut x = rect.left.round() as i32;
    let y = rect.top.round() as i32;
    let mut w = (rect.right - rect.left).round() as i32;
    let h = (rect.bottom - rect.top).round() as u32;
    if w <= 0 || h == 0 { return; }

    // Clamp to screen bounds so the hit window is always clickable.
    // NOTE: w can go negative after clamping (pet mostly off-screen), so the
    // `w <= 0` guard below is critical before the `w as u32` cast.
    if let Some(monitor) = app.primary_monitor().ok().flatten() {
        let screen_w = monitor.size().width as i32;
        if x < 0 { w += x; x = 0; }              // trim left overflow
        if x + w > screen_w { w = screen_w - x; } // trim right overflow
    }
    if w <= 0 { return; }

    let _ = hit_win.set_position(PhysicalPosition::new(x, y));
    let _ = hit_win.set_size(PhysicalSize::new(w as u32, h));
}

pub fn get_pet_bounds(app: &AppHandle) -> Option<WindowBounds> {
    let pet  = app.get_webview_window("pet")?;
    let pos  = pet.outer_position().ok()?;
    let size = pet.outer_size().ok()?;
    Some(WindowBounds {
        x: pos.x, y: pos.y,
        width: size.width, height: size.height,
    })
}

pub fn show_hit_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("hit") {
        let _ = w.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_rect_has_positive_dimensions() {
        let bounds = WindowBounds { x: 100, y: 200, width: 200, height: 200 };
        let rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        assert!(rect.right > rect.left, "width must be positive");
        assert!(rect.bottom > rect.top, "height must be positive");
    }

    #[test]
    fn test_hit_rect_inside_window() {
        let bounds = WindowBounds { x: 0, y: 0, width: 200, height: 200 };
        let rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        assert!(rect.left >= -10.0, "left should be near window");
        assert!(rect.right <= 210.0, "right should be near window");
    }

    #[test]
    fn test_wide_hitbox_wider_than_default() {
        let bounds = WindowBounds { x: 0, y: 0, width: 200, height: 200 };
        let default_rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        let wide_rect    = compute_hit_rect(&bounds, &HitBox::WIDE);
        assert!(
            (wide_rect.right - wide_rect.left) > (default_rect.right - default_rect.left),
            "WIDE hitbox should produce wider rect"
        );
    }
}
