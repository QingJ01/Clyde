use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use crate::util::MutexExt;

pub type SharedPrefs = Arc<Mutex<Prefs>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)] pub x: i32,
    #[serde(default)] pub y: i32,
    #[serde(default = "default_size")] pub size: String,
    #[serde(default)] pub mini_mode: bool,
    #[serde(default)] pub pre_mini_x: i32,
    #[serde(default)] pub pre_mini_y: i32,
    #[serde(default = "default_lang")] pub lang: String,
    #[serde(default = "default_true")] pub show_tray: bool,
    #[serde(default)] pub auto_start_with_claude: bool,
    #[serde(default)] pub bubble_follow_pet: bool,
}

/// Default pet dimension (pixels) when bounds are unavailable.
pub const DEFAULT_PET_DIMENSION: u32 = 200;
/// Default screen size fallback when monitor info is unavailable.
pub const DEFAULT_SCREEN_SIZE: (u32, u32) = (1920, 1080);

pub fn size_to_pixels(size: &str) -> (u32, u32) {
    match size { "M" => (280, 280), "L" => (360, 360), _ => (200, 200) }
}

/// Check if mini mode is currently active. Returns false if state is unavailable.
pub fn is_mini_mode(app: &AppHandle) -> bool {
    app.try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().mini_mode)
        .unwrap_or(false)
}

fn default_size() -> String { "S".into() }
fn default_lang() -> String { "en".into() }
fn default_true() -> bool   { true }

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            x: 100, y: 100, size: "S".into(), mini_mode: false,
            pre_mini_x: 0, pre_mini_y: 0, lang: "en".into(),
            show_tray: true, auto_start_with_claude: false, bubble_follow_pet: false,
        }
    }
}

fn prefs_path(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir()
        .unwrap_or_else(|_| dirs::home_dir()
            .map(|h| h.join(".clyde"))
            .unwrap_or_else(|| std::path::PathBuf::from(".clyde")))
        .join("clyde-prefs.json")
}

pub fn load(app: &AppHandle) -> Prefs {
    let path = prefs_path(app);
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return Prefs::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(app: &AppHandle, prefs: &Prefs) {
    let path = prefs_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = match serde_json::to_string_pretty(prefs) {
        Ok(j) => j,
        Err(e) => { eprintln!("Clyde: failed to serialize prefs: {e}"); return; }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("Clyde: failed to write prefs tmp: {e}"); return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        eprintln!("Clyde: failed to rename prefs: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_prefs_default() {
        let p = Prefs::default();
        assert_eq!(p.size, "S");
        assert_eq!(p.lang, "en");
        assert!(p.show_tray);
    }
    #[test]
    fn test_prefs_roundtrip() {
        let p = Prefs { lang: "zh".into(), size: "L".into(), ..Default::default() };
        let json = serde_json::to_string(&p).unwrap();
        let p2: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(p2.lang, "zh");
        assert_eq!(p2.size, "L");
    }
}
