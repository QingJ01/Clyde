use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition};
use crate::prefs::{self, SharedPrefs};
use crate::util::MutexExt;
use crate::state_machine::SharedState;
use crate::windows::get_pet_bounds;
use crate::{emit_state, sync_hit};

pub const SNAP_TOLERANCE: i32 = 30;
#[allow(dead_code)]
pub const PEEK_OFFSET: i32    = 25;
pub const MINI_OFFSET_RATIO: f64 = 0.486;

/// Which screen edge the pet is snapping to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapSide { Left, Right }

pub fn should_snap_to_edge(app: &AppHandle) -> Option<EdgeSnap> {
    let bounds = get_pet_bounds(app)?;
    let monitor = app.primary_monitor().ok()??;
    let screen_w = monitor.size().width as i32;
    let pet_right = bounds.x + bounds.width as i32;

    if screen_w - pet_right <= SNAP_TOLERANCE {
        Some(EdgeSnap { screen_w, width: bounds.width, side: SnapSide::Right })
    } else if bounds.x <= SNAP_TOLERANCE {
        Some(EdgeSnap { screen_w, width: bounds.width, side: SnapSide::Left })
    } else {
        None
    }
}

pub struct EdgeSnap {
    pub screen_w: i32,
    pub width: u32,
    pub side: SnapSide,
}

impl EdgeSnap {
    /// The X position where the pet hides at the edge (partially off-screen).
    pub fn hidden_x(&self) -> i32 {
        match self.side {
            SnapSide::Right => self.screen_w - (self.width as f64 * MINI_OFFSET_RATIO).round() as i32,
            SnapSide::Left  => -((self.width as f64 * (1.0 - MINI_OFFSET_RATIO)).round() as i32),
        }
    }
}

/// Exit mini mode: restore position, emit idle state, sync hit window.
/// Returns true if the pet was in mini mode and was restored.
pub fn do_exit_mini(app: &AppHandle) -> bool {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else { return false };
    let (was_mini, pre_x, pre_y) = {
        let mut p = prefs_state.lock_or_recover();
        if !p.mini_mode { return false; }
        p.mini_mode = false;
        let pos = (true, p.pre_mini_x, p.pre_mini_y);
        prefs::save(app, &p);
        pos
    };
    if !was_mini { return false; }
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.set_position(PhysicalPosition::new(pre_x, pre_y));
    }
    // Restore the real state from the state machine instead of hardcoding idle
    let (resolved, svg) = if let Some(state) = app.try_state::<SharedState>() {
        let sm = state.lock_or_recover();
        let r = sm.resolve_display_state();
        let s = sm.svg_for_state(&r);
        (r, s)
    } else {
        ("idle".into(), "clyde-idle-follow.svg".into())
    };
    emit_state(app, &resolved, &svg);
    sync_hit(app);
    true
}

/// Enter mini mode: save current position, animate to edge, emit mini state.
/// Returns true if mini mode was entered.
pub fn do_enter_mini(app: &AppHandle) -> bool {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else { return false };

    // Determine target X: from edge snap (left or right), or default to right edge
    let (hidden_x, cur_x, cur_y) = if let Some(snap) = should_snap_to_edge(app) {
        let bounds = match get_pet_bounds(app) { Some(b) => b, None => return false };
        (snap.hidden_x(), bounds.x, bounds.y)
    } else {
        // Not near any edge (triggered from tray/context menu) — default to right edge
        let monitor = match app.primary_monitor().ok().flatten() { Some(m) => m, None => return false };
        let screen_w = monitor.size().width as i32;
        let pet = match app.get_webview_window("pet") { Some(p) => p, None => return false };
        let size = pet.outer_size().unwrap_or_default();
        let pos = pet.outer_position().unwrap_or_default();
        let hx = screen_w - (size.width as f64 * MINI_OFFSET_RATIO).round() as i32;
        (hx, pos.x, pos.y)
    };

    {
        let mut p = prefs_state.lock_or_recover();
        p.pre_mini_x = cur_x;
        p.pre_mini_y = cur_y;
        p.mini_mode = true;
        prefs::save(app, &p);
    }

    emit_state(app, "mini-idle", "clyde-mini-enter.svg");
    // animate_to_x automatically syncs hit window when animation completes
    animate_to_x(app, hidden_x, 300);
    true
}

/// Animate window with parabolic arc (jump transition).
#[allow(dead_code)]
pub fn animate_parabola(app: &AppHandle, target_x: i32, target_y: i32, peak_height: i32, duration_ms: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(pet) = app.get_webview_window("pet") {
            let start_pos = pet.outer_position().unwrap_or_default();
            let sx = start_pos.x as f64;
            let sy = start_pos.y as f64;
            let tx = target_x as f64;
            let ty = target_y as f64;
            let steps = (duration_ms / 16).max(1);
            let mut tick_interval = tokio::time::interval(Duration::from_millis(16));
            for i in 1..=steps {
                tick_interval.tick().await;
                let t = i as f64 / steps as f64;
                let eased = t * (2.0 - t);
                let x = sx + (tx - sx) * eased;
                // Parabolic arc: -4 * peak * t * (t - 1)
                let arc = -4.0 * peak_height as f64 * t * (t - 1.0);
                let y = sy + (ty - sy) * eased - arc;
                let _ = pet.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
            }
        }
        sync_hit(&app);
    });
}

/// Peek in: slide to peek position (absolute, not relative — prevents drift)
pub fn peek_in(app: &AppHandle) {
    if let Some(snap) = should_snap_to_edge(app) {
        let target_x = match snap.side {
            SnapSide::Right => snap.hidden_x() - PEEK_OFFSET,
            SnapSide::Left  => snap.hidden_x() + PEEK_OFFSET,
        };
        animate_to_x(app, target_x, 200);
    }
}

/// Peek out: slide back to hidden position
pub fn peek_out(app: &AppHandle) {
    if let Some(snap) = should_snap_to_edge(app) {
        animate_to_x(app, snap.hidden_x(), 200);
    }
}

/// Animate window to target X in steps (ease-out quadratic).
/// Automatically syncs the hit window after the animation completes.
pub fn animate_to_x(app: &AppHandle, target_x: i32, duration_ms: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(pet) = app.get_webview_window("pet") {
            let start_pos = pet.outer_position().unwrap_or_default();
            let start_x = start_pos.x;
            let start_y = start_pos.y;
            if start_x == target_x {
                sync_hit(&app);
                return;
            }

            let steps = (duration_ms / 16).max(1);
            let mut interval = tokio::time::interval(Duration::from_millis(16));
            for i in 1..=steps {
                interval.tick().await;
                let t = i as f64 / steps as f64;
                let eased = t * (2.0 - t); // ease-out quad
                let x = (start_x as f64 + (target_x - start_x) as f64 * eased).round() as i32;
                let _ = pet.set_position(PhysicalPosition::new(x, start_y));
            }
        }
        // Always sync hit window after animation so the click area matches the pet
        sync_hit(&app);
    });
}
