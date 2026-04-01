use tauri::{AppHandle, Manager, WebviewWindowBuilder, WebviewUrl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::util::MutexExt;

pub type BubbleMap = Arc<Mutex<HashMap<String, BubbleEntry>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowKind {
    ApprovalRequest,
    ModeNotice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BubbleData {
    pub id: String,
    pub window_kind: WindowKind,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub suggestions: Vec<serde_json::Value>,
    pub session_id: String,
    pub is_elicitation: bool,
    // mode_notice fields
    pub mode_label: Option<String>,
    pub mode_description: Option<String>,
}

pub struct BubbleEntry {
    pub data: BubbleData,
    pub measured_height: u32,
}

const BUBBLE_WIDTH: u32 = 340;
const BUBBLE_MARGIN: u32 = 8;
const BUBBLE_GAP: u32 = 6;

pub fn show_bubble(app: &AppHandle, bubbles: &BubbleMap, data: BubbleData) -> bool {
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

    // transparent() and shadow() are only available on Windows/Linux
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.transparent(true).shadow(false);
    }

    let window = builder.build();

    match window {
        Ok(_) => {
            bubbles.lock_or_recover().insert(id, BubbleEntry { data, measured_height: 200 });
            reposition_bubbles(app, bubbles);
            true
        }
        Err(e) => {
            eprintln!("Clyde: failed to create bubble window: {e}");
            false
        }
    }
}

pub fn close_bubble(app: &AppHandle, bubbles: &BubbleMap, id: &str) {
    // Atomically remove from map first — if already removed (e.g. scopeguard + user click),
    // skip the rest to avoid double-destroy race condition.
    let removed = bubbles.lock_or_recover().remove(id).is_some();
    if !removed { return; }
    if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
        let _ = win.destroy();
    }
    reposition_bubbles(app, bubbles);
}

/// All bubble positioning uses **physical pixels** (matching get_pet_bounds,
/// PhysicalPosition, etc.). Design constants (BUBBLE_WIDTH, BUBBLE_MARGIN)
/// are scaled by DPI at use sites.
pub fn reposition_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let mut entries: Vec<(String, u32)> = bubbles.lock_or_recover()
        .iter().map(|(id, e)| (id.clone(), e.measured_height)).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() { return; }

    let scale = get_scale(app);
    let bw = scaled(BUBBLE_WIDTH, scale);
    let bm = scaled(BUBBLE_MARGIN, scale);
    let bg = scaled(BUBBLE_GAP, scale);

    let (screen_w, _) = get_work_area(app);
    let (anchor_x, anchor_y, pet_w, pet_h) = get_pet_anchor(app);

    let total_h: i32 = entries.iter().map(|(_, h)| *h as i32 + bg).sum();
    let stack_above = anchor_y >= total_h + bm;

    if stack_above {
        let mut y_bottom = anchor_y;
        for (id, height) in &entries {
            let label = format!("bubble-{id}");
            if let Some(win) = app.get_webview_window(&label) {
                let x = center_bubble_x(anchor_x, pet_w, screen_w, bw, bm);
                let y = y_bottom - *height as i32 - bg;
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                y_bottom = y;
            }
        }
    } else {
        let mut y_top = anchor_y + pet_h as i32 + bg;
        for (id, height) in &entries {
            let label = format!("bubble-{id}");
            if let Some(win) = app.get_webview_window(&label) {
                let x = center_bubble_x(anchor_x, pet_w, screen_w, bw, bm);
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y_top));
                y_top += *height as i32 + bg;
            }
        }
    }
}

fn center_bubble_x(pet_x: i32, pet_size: u32, screen_w: u32, bw: i32, bm: i32) -> i32 {
    let center = pet_x + pet_size as i32 / 2;
    let x = center - bw / 2;
    x.max(bm).min(screen_w as i32 - bw - bm)
}

/// Returns (x, y, width, height) of the pet window for bubble positioning.
fn get_pet_anchor(app: &AppHandle) -> (i32, i32, u32, u32) {
    if let Some(bounds) = crate::windows::get_pet_bounds(app) {
        (bounds.x, bounds.y, bounds.width, bounds.height)
    } else {
        let (sw, sh) = get_work_area(app);
        let scale = get_scale(app);
        let bw = scaled(BUBBLE_WIDTH, scale);
        let bm = scaled(BUBBLE_MARGIN, scale);
        (sw as i32 - bw - bm, sh as i32 - 200, 200, 200)
    }
}

#[cfg(test)]
pub fn bubble_position_for_index(screen_w: u32, screen_h: u32, index: u32, bubble_height: u32) -> (u32, u32) {
    let x = screen_w.saturating_sub(BUBBLE_WIDTH + BUBBLE_MARGIN);
    let y = screen_h.saturating_sub(BUBBLE_MARGIN + bubble_height + index * (bubble_height + BUBBLE_GAP));
    (x, y)
}

fn initial_bubble_position(app: &AppHandle, bubbles: &BubbleMap) -> (u32, u32) {
    let scale = get_scale(app);
    let bg = scaled(BUBBLE_GAP, scale);
    let bw = scaled(BUBBLE_WIDTH, scale);
    let bm = scaled(BUBBLE_MARGIN, scale);
    let (screen_w, _) = get_work_area(app);
    let (pet_x, pet_y, pet_w, _pet_h) = get_pet_anchor(app);
    let count = bubbles.lock_or_recover().len() as i32;
    let x = center_bubble_x(pet_x, pet_w, screen_w, bw, bm);
    let y = pet_y - (count + 1) * (scaled(200, scale) + bg);
    (x.max(0) as u32, y.max(0) as u32)
}

fn get_work_area(app: &AppHandle) -> (u32, u32) {
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(Some(monitor)) = pet.current_monitor() {
            return (monitor.size().width, monitor.size().height);
        }
    }
    app.primary_monitor()
        .ok().flatten()
        .map(|m| (m.size().width, m.size().height))
        .unwrap_or(crate::prefs::DEFAULT_SCREEN_SIZE)
}

fn get_scale(app: &AppHandle) -> f64 {
    app.get_webview_window("pet")
        .and_then(|p| p.scale_factor().ok())
        .unwrap_or(1.0)
}

fn scaled(logical: u32, scale: f64) -> i32 {
    (logical as f64 * scale).round() as i32
}

#[tauri::command]
pub fn get_bubble_data(
    bubbles: tauri::State<BubbleMap>,
    id: String,
) -> Option<BubbleData> {
    bubbles.lock_or_recover().get(&id).map(|e| e.data.clone())
}

#[tauri::command]
pub fn bubble_height_measured(
    app: AppHandle,
    bubbles: tauri::State<BubbleMap>,
    id: String,
    height: u32,
) {
    if let Some(entry) = bubbles.lock_or_recover().get_mut(&id) {
        entry.measured_height = height;
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
}
