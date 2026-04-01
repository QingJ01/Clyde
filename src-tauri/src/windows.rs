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
    // A generous interaction area so dragging works across the full pet,
    // not just the sprite's narrow torso hotspot.
    pub const INTERACTIVE: HitBox = HitBox { x: -10, y: -16, w: 35, h: 35 };
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

    // Clamp to current monitor bounds so the hit window is always clickable.
    let mon = get_pet_monitor(app);
    let mon_left = mon.x;
    let mon_right = mon.x + mon.width as i32;
    if x < mon_left { w -= mon_left - x; x = mon_left; }
    if x + w > mon_right { w = mon_right - x; }
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

/// Get the monitor the pet window is currently on.
/// Falls back to primary monitor, then to default screen size.
/// Get the monitor the pet is on, in **physical pixels**.
/// Matches get_pet_bounds(), outer_position(), cursor_position(), etc.
/// Only drag_move needs logical coords — it converts separately.
pub fn get_pet_monitor(app: &AppHandle) -> MonitorBounds {
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(Some(monitor)) = pet.current_monitor() {
            let pos = monitor.position();
            let size = monitor.size();
            return MonitorBounds {
                x: pos.x, y: pos.y,
                width: size.width, height: size.height,
            };
        }
    }
    if let Some(monitor) = app.primary_monitor().ok().flatten() {
        let pos = monitor.position();
        let size = monitor.size();
        return MonitorBounds {
            x: pos.x, y: pos.y,
            width: size.width, height: size.height,
        };
    }
    MonitorBounds { x: 0, y: 0, width: 1920, height: 1080 }
}

#[derive(Debug, Clone, Copy)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Construct pet bounds from persisted prefs (used at startup before window is fully rendered).
pub fn startup_pet_bounds(prefs: &crate::prefs::Prefs) -> WindowBounds {
    let (width, height) = crate::prefs::size_to_pixels(&prefs.size);
    WindowBounds { x: prefs.x, y: prefs.y, width, height }
}

/// Construct bounds with same position but new size (used after set_size to avoid race).
pub fn resized_pet_bounds(current: &WindowBounds, width: u32, height: u32) -> WindowBounds {
    WindowBounds { x: current.x, y: current.y, width, height }
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

    #[test]
    fn test_interactive_hitbox_covers_most_of_pet_window() {
        let bounds = WindowBounds { x: 0, y: 0, width: 200, height: 200 };
        let rect = compute_hit_rect(&bounds, &HitBox::INTERACTIVE);
        assert!(rect.left <= 5.0, "interactive hit area should start near left edge");
        assert!(rect.top <= 5.0, "interactive hit area should start near top edge");
        assert!(rect.right >= 195.0, "interactive hit area should reach near right edge");
        assert!(rect.bottom >= 195.0, "interactive hit area should reach near bottom edge");
    }

    #[test]
    fn test_startup_bounds_use_stored_prefs() {
        let prefs = crate::prefs::Prefs {
            x: 100, y: 100, size: "L".into(), ..Default::default()
        };
        let bounds = startup_pet_bounds(&prefs);
        assert_eq!(bounds.x, 100);
        assert_eq!(bounds.y, 100);
        assert_eq!(bounds.width, 360);
        assert_eq!(bounds.height, 360);
    }

    #[test]
    fn test_resized_bounds_keep_position() {
        let current = WindowBounds { x: 320, y: 180, width: 200, height: 200 };
        let resized = resized_pet_bounds(&current, 360, 360);
        assert_eq!(resized.x, 320);
        assert_eq!(resized.y, 180);
        assert_eq!(resized.width, 360);
        assert_eq!(resized.height, 360);
    }
}
