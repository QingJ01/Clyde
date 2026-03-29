use tauri::{AppHandle, Manager, WebviewWindowBuilder, WebviewUrl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type BubbleMap = Arc<Mutex<HashMap<String, BubbleEntry>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BubbleData {
    pub id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub suggestions: Vec<serde_json::Value>,
    pub session_id: String,
    pub is_elicitation: bool,
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

    let (x, y) = initial_bubble_position(app, bubbles);

    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("")
        .inner_size(BUBBLE_WIDTH as f64, 200.0)
        .position(x as f64, y as f64)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(true)
        .build();

    match window {
        Ok(_) => {
            bubbles.lock().expect("bubble map poisoned").insert(id, BubbleEntry { data, measured_height: 200 });
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
    let removed = bubbles.lock().expect("bubble map poisoned").remove(id).is_some();
    if !removed { return; }
    if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
        let _ = win.destroy();
    }
    reposition_bubbles(app, bubbles);
}

pub fn reposition_bubbles(app: &AppHandle, bubbles: &BubbleMap) {
    let mut entries: Vec<(String, u32)> = bubbles.lock().expect("bubble map poisoned")
        .iter().map(|(id, e)| (id.clone(), e.measured_height)).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() { return; }

    let (screen_w, screen_h) = get_work_area(app);
    let mut y_bottom = screen_h.saturating_sub(BUBBLE_MARGIN);

    for (id, height) in &entries {
        let label = format!("bubble-{id}");
        if let Some(win) = app.get_webview_window(&label) {
            let x = screen_w.saturating_sub(BUBBLE_WIDTH + BUBBLE_MARGIN);
            let y = y_bottom.saturating_sub(*height);
            let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
            y_bottom = y.saturating_sub(BUBBLE_GAP);
        }
    }
}

fn initial_bubble_position(app: &AppHandle, bubbles: &BubbleMap) -> (u32, u32) {
    let (screen_w, screen_h) = get_work_area(app);
    let count = bubbles.lock().expect("bubble map poisoned").len() as u32;
    let x = screen_w.saturating_sub(BUBBLE_WIDTH + BUBBLE_MARGIN);
    let y = screen_h.saturating_sub(BUBBLE_MARGIN + 200 + count * (200 + BUBBLE_GAP));
    (x, y)
}

fn get_work_area(app: &AppHandle) -> (u32, u32) {
    app.primary_monitor()
        .ok().flatten()
        .map(|m| (m.size().width, m.size().height))
        .unwrap_or((1920, 1080))
}

#[tauri::command]
pub fn get_bubble_data(
    bubbles: tauri::State<BubbleMap>,
    id: String,
) -> Option<BubbleData> {
    bubbles.lock().expect("bubble map poisoned").get(&id).map(|e| e.data.clone())
}

#[tauri::command]
pub fn bubble_height_measured(
    app: AppHandle,
    bubbles: tauri::State<BubbleMap>,
    id: String,
    height: u32,
) {
    if let Some(entry) = bubbles.lock().expect("bubble map poisoned").get_mut(&id) {
        entry.measured_height = height;
    }
    reposition_bubbles(&app, &bubbles);
}
