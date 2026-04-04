use crate::util::MutexExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{
    window::Color, AppHandle, LogicalSize, Manager, Size, WebviewUrl, WebviewWindowBuilder,
};

pub type BubbleMap = Arc<Mutex<HashMap<String, BubbleEntry>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowKind {
    ApprovalRequest,
    ModeNotice,
    UpdateNotice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BubbleData {
    pub id: String,
    pub window_kind: WindowKind,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub suggestions: Vec<serde_json::Value>,
    pub session_id: String,
    pub agent_label: String,
    pub session_summary: String,
    pub session_project: String,
    pub session_short_id: String,
    pub is_elicitation: bool,
    pub elicitation_message: Option<String>,
    pub elicitation_schema: Option<serde_json::Value>,
    pub elicitation_mode: Option<String>,
    pub elicitation_url: Option<String>,
    pub elicitation_server_name: Option<String>,
    // mode_notice fields
    pub mode_label: Option<String>,
    pub mode_description: Option<String>,
    // update_notice fields
    pub update_version: Option<String>,
    pub update_url: Option<String>,
    pub update_notes: Option<String>,
    pub update_lang: Option<String>,
}

pub struct BubbleEntry {
    pub data: BubbleData,
    pub measured_height: u32,
}

const BUBBLE_WIDTH: u32 = 340;
const BUBBLE_MARGIN: u32 = 8;
const BUBBLE_GAP: u32 = 6;

struct BubbleAnchor {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn show_bubble(app: &AppHandle, bubbles: &BubbleMap, data: BubbleData) -> bool {
    // Auto-restore from tray when a permission request arrives
    if crate::is_hidden(app) {
        crate::do_show_from_tray(app);
    }

    let id = data.id.clone();
    let label = format!("bubble-{}", id);
    let url = format!("src/windows/bubble/index.html?entry_id={id}");

    let (x_phys, y_phys) = initial_bubble_position(app, bubbles);
    let scale = get_scale(app);
    // .position() and .inner_size() take logical coordinates
    let x_log = x_phys as f64 / scale;
    let y_log = y_phys as f64 / scale;

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("")
        .inner_size(BUBBLE_WIDTH as f64, 200.0)
        .position(x_log, y_log)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(true);

    builder = builder.shadow(false);
    // transparent() is not available on macOS (handled by macOSPrivateApi)
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.transparent(true);
    }

    let window = builder.build();

    match window {
        Ok(window) => {
            crate::macos_spaces::apply_space_follow(&window);
            let _ = window.set_shadow(false);
            let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
            let phys_placeholder = (200.0 * scale).round() as u32;
            bubbles.lock_or_recover().insert(
                id,
                BubbleEntry {
                    data,
                    measured_height: phys_placeholder,
                },
            );
            reposition_bubbles(app, bubbles);
            true
        }
        Err(e) => {
            eprintln!("Clyde: failed to create bubble window: {e}");
            false
        }
    }
}

/// Hide all open bubble windows (without destroying them).
pub fn hide_all_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let ids: Vec<String> = bubbles.lock_or_recover().keys().cloned().collect();
    for id in ids {
        if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
            let _ = win.hide();
        }
    }
}

/// Show all open bubble windows.
pub fn show_all_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let ids: Vec<String> = bubbles.lock_or_recover().keys().cloned().collect();
    for id in ids {
        if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
            let _ = win.show();
        }
    }
}

pub fn close_bubble(app: &AppHandle, bubbles: &BubbleMap, id: &str) {
    // Atomically remove from map first — if already removed (e.g. scopeguard + user click),
    // skip the rest to avoid double-destroy race condition.
    let removed = bubbles.lock_or_recover().remove(id).is_some();
    if !removed {
        return;
    }
    if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
        let _ = win.destroy();
    }
    reposition_bubbles(app, bubbles);
}

pub fn close_mode_notice_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let ids: Vec<String> = {
        let map = bubbles.lock_or_recover();
        map.iter()
            .filter(|(_, entry)| matches!(entry.data.window_kind, WindowKind::ModeNotice))
            .map(|(id, _)| id.clone())
            .collect()
    };
    for id in ids {
        close_bubble(app, bubbles, &id);
    }
}

/// All bubble positioning uses **physical pixels** (matching get_pet_bounds,
/// PhysicalPosition, etc.). `measured_height` is stored in physical pixels
/// (converted from logical on receipt). Design constants are scaled by DPI.
pub fn reposition_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let mut entries: Vec<(String, u32)> = bubbles
        .lock_or_recover()
        .iter()
        .map(|(id, e)| (id.clone(), e.measured_height))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() {
        return;
    }

    let scale = get_scale(app);
    let margin = (BUBBLE_MARGIN as f64 * scale).round() as i32;
    let gap = (BUBBLE_GAP as f64 * scale).round() as i32;

    let monitor = get_work_area(app);
    let anchor = get_pet_anchor(app, &monitor);

    // Total height needed for all bubbles (measured_height is already physical)
    let total_h: i32 = entries.iter().map(|(_, h)| *h as i32 + gap).sum();

    let space_above = (anchor.y - monitor.y).max(0);
    let space_below =
        (monitor.y + monitor.height as i32 - (anchor.y + anchor.height as i32)).max(0);
    let stack_above =
        space_above >= total_h + margin || (space_above >= space_below && space_above > 0);

    if stack_above {
        // Stack upward from pet's top edge
        let mut y_bottom = anchor.y;
        for (id, height) in &entries {
            let label = format!("bubble-{id}");
            if let Some(win) = app.get_webview_window(&label) {
                let x = center_bubble_x(anchor.x, anchor.width, &monitor, scale);
                let desired_y = y_bottom - *height as i32 - gap;
                let y = desired_y.max(monitor.y + margin);
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                y_bottom = y;
            }
        }
    } else {
        // Stack downward from pet's bottom edge
        let mut y_top = anchor.y + anchor.height as i32 + gap;
        for (id, height) in &entries {
            let label = format!("bubble-{id}");
            if let Some(win) = app.get_webview_window(&label) {
                let x = center_bubble_x(anchor.x, anchor.width, &monitor, scale);
                let max_y =
                    monitor.y + monitor.height as i32 - *height as i32 - margin;
                let y = y_top.min(max_y.max(monitor.y + margin));
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                y_top = y + *height as i32 + gap;
            }
        }
    }
}

/// Calculate X position: center bubble relative to pet, clamped to screen.
/// All coordinates are physical pixels; `scale` is the DPI factor.
fn center_bubble_x(pet_x: i32, pet_width: u32, monitor: &crate::windows::MonitorArea, scale: f64) -> i32 {
    let bw = (BUBBLE_WIDTH as f64 * scale).round() as i32;
    let margin = (BUBBLE_MARGIN as f64 * scale).round() as i32;
    let center = pet_x + pet_width as i32 / 2;
    let x = center - bw / 2;
    let min_x = monitor.x + margin;
    let max_x = monitor.x + monitor.width as i32 - bw - margin;
    x.max(min_x).min(max_x.max(min_x))
}

/// Get pet window position and size as bubble anchor point.
fn get_pet_anchor(app: &AppHandle, monitor: &crate::windows::MonitorArea) -> BubbleAnchor {
    if let Some(bounds) = crate::windows::get_pet_bounds(app) {
        BubbleAnchor {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
    } else {
        // Fallback: bottom-right corner
        BubbleAnchor {
            x: monitor.x + monitor.width as i32 - 200 - BUBBLE_MARGIN as i32,
            y: monitor.y + monitor.height as i32 - 200,
            width: 200,
            height: 200,
        }
    }
}

#[cfg(test)]
pub fn bubble_position_for_index(
    screen_w: u32,
    screen_h: u32,
    index: u32,
    bubble_height: u32,
) -> (u32, u32) {
    let x = screen_w.saturating_sub(BUBBLE_WIDTH + BUBBLE_MARGIN);
    let y = screen_h
        .saturating_sub(BUBBLE_MARGIN + bubble_height + index * (bubble_height + BUBBLE_GAP));
    (x, y)
}

fn initial_bubble_position(app: &AppHandle, bubbles: &BubbleMap) -> (i32, i32) {
    let scale = get_scale(app);
    let margin = (BUBBLE_MARGIN as f64 * scale).round() as i32;
    let gap = (BUBBLE_GAP as f64 * scale).round() as i32;
    let placeholder_h = (200.0 * scale).round() as i32;
    let monitor = get_work_area(app);
    let anchor = get_pet_anchor(app, &monitor);
    let count = bubbles.lock_or_recover().len() as u32;
    let x = center_bubble_x(anchor.x, anchor.width, &monitor, scale);
    let min_y = monitor.y + margin;
    let y = (anchor.y - (count as i32 + 1) * (placeholder_h + gap)).max(min_y);
    (x, y)
}

fn get_work_area(app: &AppHandle) -> crate::windows::MonitorArea {
    if let Some(bounds) = crate::windows::get_pet_bounds(app) {
        if let Some(monitor) = crate::windows::monitor_for_bounds(app, &bounds) {
            return monitor;
        }
    }
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(Some(monitor)) = pet.current_monitor() {
            return crate::windows::monitor_area(&monitor);
        }
    }
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| crate::windows::monitor_area(&monitor))
        .unwrap_or_else(|| {
            let (width, height) = crate::prefs::DEFAULT_SCREEN_SIZE;
            crate::windows::MonitorArea {
                key: "fallback".into(),
                x: 0,
                y: 0,
                width,
                height,
            }
        })
}

fn get_scale(app: &AppHandle) -> f64 {
    crate::windows::pet_scale_factor(app)
}

#[tauri::command]
pub fn get_bubble_data(bubbles: tauri::State<BubbleMap>, id: String) -> Option<BubbleData> {
    bubbles.lock_or_recover().get(&id).map(|e| e.data.clone())
}

#[tauri::command]
pub fn bubble_height_measured(
    app: AppHandle,
    bubbles: tauri::State<BubbleMap>,
    id: String,
    height: u32,
) {
    // height from frontend is in logical (CSS) pixels; convert to physical for
    // consistent positioning math in reposition_bubbles.
    let scale = get_scale(&app);
    let phys_height = (height as f64 * scale).round() as u32;
    if let Some(entry) = bubbles.lock_or_recover().get_mut(&id) {
        entry.measured_height = phys_height;
    }
    // set_size takes logical pixels — use the original height
    if let Some(window) = app.get_webview_window(&format!("bubble-{id}")) {
        let _ = window.set_size(Size::Logical(LogicalSize::new(
            BUBBLE_WIDTH as f64,
            height.max(160) as f64,
        )));
    }
    reposition_bubbles(&app, &bubbles);
}

/// Dismiss a bubble (used by ModeNotice OK button). Cleans up BubbleMap properly.
#[tauri::command]
pub fn dismiss_bubble(
    app: AppHandle,
    bubbles: tauri::State<BubbleMap>,
    id: String,
) {
    close_bubble(&app, &bubbles, &id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bubble_position_first() {
        let (x, y) = bubble_position_for_index(1920, 1080, 0, 200);
        assert_eq!(x, 1920 - BUBBLE_WIDTH - BUBBLE_MARGIN); // 1572
        assert_eq!(y, 1080 - BUBBLE_MARGIN - 200); // 872
    }

    #[test]
    fn test_bubble_position_stacking() {
        let (_, y1) = bubble_position_for_index(1920, 1080, 0, 200);
        let (_, y2) = bubble_position_for_index(1920, 1080, 1, 200);
        assert!(y2 < y1, "second bubble should be above first");
        assert_eq!(y1 - y2, 200 + BUBBLE_GAP);
    }

    #[test]
    fn test_bubble_position_no_underflow() {
        // Many bubbles shouldn't underflow
        let (_, y) = bubble_position_for_index(1920, 1080, 100, 200);
        // saturating_sub prevents underflow, y should be 0
        assert_eq!(y, 0);
    }

    #[test]
    fn test_center_bubble_x_respects_monitor_origin() {
        let monitor = crate::windows::MonitorArea {
            key: "secondary".into(),
            x: 2560,
            y: 0,
            width: 1920,
            height: 1080,
        };

        let scale = 1.0;
        let x = center_bubble_x(2800, 360, &monitor, scale);
        assert!(x >= monitor.x + BUBBLE_MARGIN as i32);
        assert!(x <= monitor.x + monitor.width as i32 - BUBBLE_WIDTH as i32 - BUBBLE_MARGIN as i32);
    }

    #[test]
    fn test_center_bubble_x_handles_negative_monitor_origin() {
        let monitor = crate::windows::MonitorArea {
            key: "left".into(),
            x: -1728,
            y: 0,
            width: 1728,
            height: 1117,
        };

        let scale = 1.0;
        let x = center_bubble_x(-1500, 360, &monitor, scale);
        assert!(x >= monitor.x + BUBBLE_MARGIN as i32);
        assert!(x <= monitor.x + monitor.width as i32 - BUBBLE_WIDTH as i32 - BUBBLE_MARGIN as i32);
    }

    #[test]
    fn test_center_bubble_x_hidpi() {
        // On a 2x HiDPI display, physical coordinates are doubled
        let monitor = crate::windows::MonitorArea {
            key: "retina".into(),
            x: 0,
            y: 0,
            width: 3840, // 1920 logical * 2
            height: 2160,
        };

        let scale = 2.0;
        let bw_phys = (BUBBLE_WIDTH as f64 * scale).round() as i32; // 680
        let margin_phys = (BUBBLE_MARGIN as f64 * scale).round() as i32; // 16
        let x = center_bubble_x(1800, 400, &monitor, scale);
        assert!(x >= monitor.x + margin_phys);
        assert!(x <= monitor.x + monitor.width as i32 - bw_phys - margin_phys);
    }
}
