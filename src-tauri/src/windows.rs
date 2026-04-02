use tauri::window::Monitor;
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

#[derive(Debug, Clone)]
pub struct MonitorArea {
    pub key: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl HitBox {
    pub const DEFAULT: HitBox = HitBox {
        x: -1,
        y: 5,
        w: 17,
        h: 12,
    };
    // A generous interaction area so dragging works across the full pet,
    // not just the sprite's narrow torso hotspot.
    pub const INTERACTIVE: HitBox = HitBox {
        x: -10,
        y: -16,
        w: 35,
        h: 35,
    };
    #[allow(dead_code)]
    pub const SLEEPING: HitBox = HitBox {
        x: -2,
        y: 9,
        w: 19,
        h: 7,
    };
    #[allow(dead_code)]
    pub const WIDE: HitBox = HitBox {
        x: -3,
        y: 3,
        w: 21,
        h: 14,
    };
}

pub fn compute_hit_rect(bounds: &WindowBounds, hb: &HitBox) -> HitRect {
    let obj_x = bounds.x as f64 + bounds.width as f64 * OBJ_OFF_X;
    let obj_y = bounds.y as f64 + bounds.height as f64 * OBJ_OFF_Y;
    let obj_w = bounds.width as f64 * OBJ_SCALE_W;
    let obj_h = bounds.height as f64 * OBJ_SCALE_H;

    let scale = obj_w.min(obj_h) / 45.0;
    let offset_x = obj_x + (obj_w - 45.0 * scale) / 2.0;
    let offset_y = obj_y + (obj_h - 45.0 * scale) / 2.0;

    HitRect {
        left: offset_x + (hb.x as f64 + 15.0) * scale,
        top: offset_y + (hb.y as f64 + 25.0) * scale,
        right: offset_x + (hb.x as f64 + 15.0 + hb.w as f64) * scale,
        bottom: offset_y + (hb.y as f64 + 25.0 + hb.h as f64) * scale,
    }
}

fn interactive_rect(bounds: &WindowBounds) -> HitRect {
    HitRect {
        left: bounds.x as f64,
        top: bounds.y as f64,
        right: (bounds.x + bounds.width as i32) as f64,
        bottom: (bounds.y + bounds.height as i32) as f64,
    }
}

fn sync_rect_for_hitbox(bounds: &WindowBounds, hb: &HitBox) -> HitRect {
    if hb.x == HitBox::INTERACTIVE.x
        && hb.y == HitBox::INTERACTIVE.y
        && hb.w == HitBox::INTERACTIVE.w
        && hb.h == HitBox::INTERACTIVE.h
    {
        interactive_rect(bounds)
    } else {
        compute_hit_rect(bounds, hb)
    }
}

pub fn monitor_key(monitor: &Monitor) -> String {
    if let Some(name) = monitor.name().filter(|name| !name.is_empty()) {
        return name.clone();
    }
    let pos = monitor.position();
    let size = monitor.size();
    format!("{}:{}:{}:{}", pos.x, pos.y, size.width, size.height)
}

pub fn monitor_area(monitor: &Monitor) -> MonitorArea {
    let pos = monitor.position();
    let size = monitor.size();
    MonitorArea {
        key: monitor_key(monitor),
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    }
}

fn rect_intersection_area(a: &WindowBounds, b: &MonitorArea) -> i64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    let width = (right - left).max(0) as i64;
    let height = (bottom - top).max(0) as i64;
    width * height
}

fn rect_center_distance_sq(a: &WindowBounds, b: &MonitorArea) -> i64 {
    let ax = a.x as i64 + a.width as i64 / 2;
    let ay = a.y as i64 + a.height as i64 / 2;
    let bx = b.x as i64 + b.width as i64 / 2;
    let by = b.y as i64 + b.height as i64 / 2;
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

pub fn monitor_for_bounds(app: &AppHandle, bounds: &WindowBounds) -> Option<MonitorArea> {
    let monitors = available_monitor_areas(app)?;
    best_monitor_for_bounds(bounds, &monitors).cloned()
}

pub fn available_monitor_areas(app: &AppHandle) -> Option<Vec<MonitorArea>> {
    let pet = app.get_webview_window("pet")?;
    let monitors = pet.available_monitors().ok()?;
    Some(
        monitors
            .into_iter()
            .map(|monitor| monitor_area(&monitor))
            .collect(),
    )
}

pub fn current_monitor_for_pet(app: &AppHandle) -> Option<MonitorArea> {
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(Some(monitor)) = pet.current_monitor() {
            return Some(monitor_area(&monitor));
        }
    }
    get_pet_bounds(app).and_then(|bounds| monitor_for_bounds(app, &bounds))
}

fn best_monitor_for_bounds<'a>(
    bounds: &WindowBounds,
    monitors: &'a [MonitorArea],
) -> Option<&'a MonitorArea> {
    monitors.iter().max_by(|a, b| {
        let area_a = rect_intersection_area(bounds, a);
        let area_b = rect_intersection_area(bounds, b);
        area_a.cmp(&area_b).then_with(|| {
            rect_center_distance_sq(bounds, b).cmp(&rect_center_distance_sq(bounds, a))
        })
    })
}

pub fn center_window_in_monitor(width: u32, height: u32, monitor: &MonitorArea) -> (i32, i32) {
    let x = monitor.x + ((monitor.width as i32 - width as i32).max(0) / 2);
    let y = monitor.y + ((monitor.height as i32 - height as i32).max(0) / 2);
    (x, y)
}

pub fn startup_position_with_monitors(
    bounds: &WindowBounds,
    monitors: &[MonitorArea],
    min_visible: i32,
) -> (i32, i32) {
    let Some(monitor) = best_monitor_for_bounds(bounds, monitors) else {
        return (bounds.x, bounds.y);
    };

    if rect_intersection_area(bounds, monitor) > 0 {
        clamp_window_to_monitor(
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            monitor,
            min_visible,
        )
    } else {
        center_window_in_monitor(bounds.width, bounds.height, monitor)
    }
}

pub fn startup_position_for_bounds(
    app: &AppHandle,
    bounds: &WindowBounds,
    min_visible: i32,
) -> (i32, i32) {
    let Some(monitors) = available_monitor_areas(app) else {
        return (bounds.x, bounds.y);
    };
    startup_position_with_monitors(bounds, &monitors, min_visible)
}

pub fn clamp_window_to_monitor(
    mut x: i32,
    mut y: i32,
    width: u32,
    height: u32,
    monitor: &MonitorArea,
    min_visible: i32,
) -> (i32, i32) {
    let left = monitor.x + min_visible - width as i32;
    let right = monitor.x + monitor.width as i32 - min_visible;
    let top = monitor.y;
    let bottom = monitor.y + monitor.height as i32 - min_visible.min(height as i32);

    if left > right {
        x = monitor.x;
    } else {
        x = x.max(left).min(right);
    }
    if top > bottom {
        y = monitor.y;
    } else {
        y = y.max(top).min(bottom);
    }
    (x, y)
}

pub fn sync_hit_window(app: &AppHandle, pet_bounds: &WindowBounds, hb: &HitBox) {
    let hit_win = match app.get_webview_window("hit") {
        Some(w) => w,
        None => return,
    };
    let rect = sync_rect_for_hitbox(pet_bounds, hb);
    let mut x = rect.left.round() as i32;
    let mut y = rect.top.round() as i32;
    let mut w = (rect.right - rect.left).round() as i32;
    let mut h = (rect.bottom - rect.top).round() as i32;
    if w <= 0 || h <= 0 {
        return;
    }

    // Clamp to screen bounds so the hit window is always clickable.
    // NOTE: w can go negative after clamping (pet mostly off-screen), so the
    // `w <= 0` guard below is critical before the `w as u32` cast.
    if let Some(monitor) = monitor_for_bounds(app, pet_bounds) {
        let screen_left = monitor.x;
        let screen_top = monitor.y;
        let screen_right = monitor.x + monitor.width as i32;
        let screen_bottom = monitor.y + monitor.height as i32;
        if x < screen_left {
            w -= screen_left - x;
            x = screen_left;
        }
        if y < screen_top {
            h -= screen_top - y;
            y = screen_top;
        }
        if x + w > screen_right {
            w = screen_right - x;
        }
        if y + h > screen_bottom {
            h = screen_bottom - y;
        }
    }
    if w <= 0 || h <= 0 {
        return;
    }

    let _ = hit_win.set_position(PhysicalPosition::new(x, y));
    let _ = hit_win.set_size(PhysicalSize::new(w as u32, h as u32));
}

pub fn get_pet_bounds(app: &AppHandle) -> Option<WindowBounds> {
    let pet = app.get_webview_window("pet")?;
    let pos = pet.outer_position().ok()?;
    let size = pet.outer_size().ok()?;
    Some(WindowBounds {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
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

pub fn hide_hit_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("hit") {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_rect_has_positive_dimensions() {
        let bounds = WindowBounds {
            x: 100,
            y: 200,
            width: 200,
            height: 200,
        };
        let rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        assert!(rect.right > rect.left, "width must be positive");
        assert!(rect.bottom > rect.top, "height must be positive");
    }

    #[test]
    fn test_hit_rect_inside_window() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        };
        let rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        assert!(rect.left >= -10.0, "left should be near window");
        assert!(rect.right <= 210.0, "right should be near window");
    }

    #[test]
    fn test_wide_hitbox_wider_than_default() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        };
        let default_rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
        let wide_rect = compute_hit_rect(&bounds, &HitBox::WIDE);
        assert!(
            (wide_rect.right - wide_rect.left) > (default_rect.right - default_rect.left),
            "WIDE hitbox should produce wider rect"
        );
    }

    #[test]
    fn test_interactive_hitbox_covers_most_of_pet_window() {
        let bounds = WindowBounds {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        };
        let rect = sync_rect_for_hitbox(&bounds, &HitBox::INTERACTIVE);
        assert_eq!(rect.left, 0.0);
        assert_eq!(rect.top, 0.0);
        assert_eq!(rect.right, 200.0);
        assert_eq!(rect.bottom, 200.0);
    }

    #[test]
    fn test_clamp_window_to_monitor_uses_monitor_origin() {
        let monitor = MonitorArea {
            key: "secondary".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, y) = clamp_window_to_monitor(3900, 100, 200, 200, &monitor, 30);
        assert_eq!(x, 3810);
        assert_eq!(y, 100);
    }

    #[test]
    fn test_center_window_in_monitor_uses_monitor_origin() {
        let monitor = MonitorArea {
            key: "secondary".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, y) = center_window_in_monitor(360, 360, &monitor);
        assert_eq!(x, 2700);
        assert_eq!(y, 360);
    }

    #[test]
    fn test_monitor_selection_prefers_intersection() {
        let bounds = WindowBounds {
            x: 2050,
            y: 50,
            width: 200,
            height: 200,
        };
        let left = MonitorArea {
            key: "left".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let right = MonitorArea {
            key: "right".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(rect_intersection_area(&bounds, &right) > rect_intersection_area(&bounds, &left));
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

    #[test]
    fn test_startup_position_centers_when_saved_bounds_are_offscreen() {
        let bounds = WindowBounds {
            x: 5000,
            y: 120,
            width: 360,
            height: 360,
        };
        let monitors = vec![
            MonitorArea {
                key: "left".into(),
                x: 0,
                y: 0,
                width: 1512,
                height: 982,
            },
            MonitorArea {
                key: "right".into(),
                x: 1512,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ];
        let (x, y) = startup_position_with_monitors(&bounds, &monitors, 120);
        assert_eq!(x, 2612);
        assert_eq!(y, 540);
    }

    #[test]
    fn test_startup_position_clamps_when_bounds_are_still_partially_visible() {
        let bounds = WindowBounds {
            x: 1400,
            y: 40,
            width: 360,
            height: 360,
        };
        let monitors = vec![MonitorArea {
            key: "main".into(),
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
        }];
        let (x, y) = startup_position_with_monitors(&bounds, &monitors, 120);
        assert_eq!(x, 1392);
        assert_eq!(y, 40);
    }
}
