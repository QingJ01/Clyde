use crate::util::MutexExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub type SharedPrefs = Arc<Mutex<Prefs>>;
pub const DEFAULT_PERMISSION_DECISION_WINDOW_SECS: u16 = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorPlacement {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub mini_side: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_size")]
    pub size: String,
    #[serde(default)]
    pub mini_mode: bool,
    #[serde(default)]
    pub pre_mini_x: i32,
    #[serde(default)]
    pub pre_mini_y: i32,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default = "default_true")]
    pub show_tray: bool,
    #[serde(default)]
    pub auto_start_with_claude: bool,
    #[serde(default)]
    pub bubble_follow_pet: bool,
    #[serde(default)]
    pub bubble_offset_x: i32,
    #[serde(default)]
    pub bubble_offset_y: i32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub lock_position: bool,
    #[serde(default)]
    pub click_through: bool,
    #[serde(default)]
    pub auto_hide_fullscreen: bool,
    #[serde(default)]
    pub auto_dnd_meetings: bool,
    #[serde(default = "default_permission_decision_window_secs")]
    pub permission_decision_window_secs: u16,
    #[serde(default)]
    pub monitor_positions: HashMap<String, MonitorPlacement>,
    #[serde(default = "default_true")]
    pub check_for_updates: bool,
    #[serde(default)]
    pub last_update_check_epoch: u64,
    #[serde(default)]
    pub dismissed_update_version: String,
}

/// Default screen size fallback when monitor info is unavailable.
pub const DEFAULT_SCREEN_SIZE: (u32, u32) = (1920, 1080);

/// Default pet window dimension (size "S").
pub const DEFAULT_PET_DIMENSION: u32 = 200;

pub fn size_to_pixels(size: &str) -> (u32, u32) {
    match size {
        "M" => (280, 280),
        "L" => (360, 360),
        _ => (200, 200),
    }
}

/// Check if mini mode is currently active. Returns false if state is unavailable.
pub fn is_mini_mode(app: &AppHandle) -> bool {
    app.try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().mini_mode)
        .unwrap_or(false)
}

fn default_size() -> String {
    "S".into()
}
fn default_lang() -> String {
    "en".into()
}
fn default_true() -> bool {
    true
}
fn default_opacity() -> f32 {
    1.0
}
fn default_permission_decision_window_secs() -> u16 {
    DEFAULT_PERMISSION_DECISION_WINDOW_SECS
}

pub fn normalize_opacity(opacity: f32) -> f32 {
    opacity.clamp(0.4, 1.0)
}

pub fn normalize_permission_decision_window_secs(secs: u16) -> u16 {
    secs.clamp(8, 120)
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            x: 100,
            y: 100,
            size: "S".into(),
            mini_mode: false,
            pre_mini_x: 0,
            pre_mini_y: 0,
            lang: "en".into(),
            show_tray: true,
            auto_start_with_claude: false,
            bubble_follow_pet: false,
            bubble_offset_x: 0,
            bubble_offset_y: 0,
            opacity: default_opacity(),
            lock_position: false,
            click_through: false,
            auto_hide_fullscreen: false,
            auto_dnd_meetings: false,
            permission_decision_window_secs: default_permission_decision_window_secs(),
            monitor_positions: HashMap::new(),
            check_for_updates: true,
            last_update_check_epoch: 0,
            dismissed_update_version: String::new(),
        }
    }
}

fn prefs_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| h.join(".clyde"))
                .unwrap_or_else(|| std::path::PathBuf::from(".clyde"))
        })
        .join("clyde-prefs.json")
}

pub fn load(app: &AppHandle) -> Prefs {
    let path = prefs_path(app);
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return Prefs::default(),
    };
    let mut prefs: Prefs = serde_json::from_str(&raw).unwrap_or_default();
    prefs.opacity = normalize_opacity(prefs.opacity);
    prefs.permission_decision_window_secs =
        normalize_permission_decision_window_secs(prefs.permission_decision_window_secs);
    prefs
}

pub fn save(app: &AppHandle, prefs: &Prefs) {
    let path = prefs_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = match serde_json::to_string_pretty(prefs) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Clyde: failed to serialize prefs: {e}");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("Clyde: failed to write prefs tmp: {e}");
        return;
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
        assert_eq!(
            p.permission_decision_window_secs,
            DEFAULT_PERMISSION_DECISION_WINDOW_SECS
        );
    }
    #[test]
    fn test_prefs_roundtrip() {
        let p = Prefs {
            lang: "zh".into(),
            size: "L".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        let p2: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(p2.lang, "zh");
        assert_eq!(p2.size, "L");
    }
    #[test]
    fn test_permission_decision_window_normalization() {
        assert_eq!(normalize_permission_decision_window_secs(3), 8);
        assert_eq!(normalize_permission_decision_window_secs(12), 12);
        assert_eq!(normalize_permission_decision_window_secs(240), 120);
    }

    #[test]
    fn test_bubble_offset_defaults_to_zero() {
        let prefs = Prefs::default();
        assert_eq!(prefs.bubble_offset_x, 0);
        assert_eq!(prefs.bubble_offset_y, 0);
    }

    #[test]
    fn test_prefs_roundtrip_preserves_bubble_offset() {
        let prefs = Prefs {
            bubble_offset_x: 48,
            bubble_offset_y: -96,
            ..Default::default()
        };

        let json = serde_json::to_string(&prefs).unwrap();
        let roundtrip: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.bubble_offset_x, 48);
        assert_eq!(roundtrip.bubble_offset_y, -96);
    }
}
