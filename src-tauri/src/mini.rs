use crate::prefs::{self, SharedPrefs};
use crate::state_machine::SharedState;
use crate::util::MutexExt;
use crate::windows::{self, get_pet_bounds, MonitorArea, WindowBounds};
use crate::{emit_state, sync_hit};
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition};

pub const SNAP_TOLERANCE: i32 = 30;
#[allow(dead_code)]
pub const PEEK_OFFSET: i32 = 25;
pub const MINI_OFFSET_RATIO: f64 = 0.486;

/// Which screen edge the pet is snapping to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapSide {
    Left,
    Right,
}

pub fn should_snap_to_edge(app: &AppHandle) -> Option<EdgeSnap> {
    let bounds = get_pet_bounds(app)?;
    edge_snap_for_bounds(app, &bounds)
}

pub fn edge_snap_for_bounds(app: &AppHandle, bounds: &WindowBounds) -> Option<EdgeSnap> {
    let monitor = windows::monitor_for_bounds(app, bounds)?;
    let screen_left = monitor.x;
    let screen_right = monitor.x + monitor.width as i32;
    let pet_right = bounds.x + bounds.width as i32;

    if screen_right - pet_right <= SNAP_TOLERANCE {
        Some(EdgeSnap {
            monitor,
            width: bounds.width,
            side: SnapSide::Right,
        })
    } else if bounds.x - screen_left <= SNAP_TOLERANCE {
        Some(EdgeSnap {
            monitor,
            width: bounds.width,
            side: SnapSide::Left,
        })
    } else {
        None
    }
}

pub struct EdgeSnap {
    pub monitor: MonitorArea,
    pub width: u32,
    pub side: SnapSide,
}

impl EdgeSnap {
    /// The X position where the pet hides at the edge (partially off-screen).
    pub fn hidden_x(&self) -> i32 {
        match self.side {
            SnapSide::Right => {
                self.monitor.x + self.monitor.width as i32
                    - (self.width as f64 * MINI_OFFSET_RATIO).round() as i32
            }
            SnapSide::Left => {
                self.monitor.x - (self.width as f64 * (1.0 - MINI_OFFSET_RATIO)).round() as i32
            }
        }
    }
}

pub fn snap_side_key(side: SnapSide) -> &'static str {
    match side {
        SnapSide::Left => "left",
        SnapSide::Right => "right",
    }
}

pub fn snap_side_from_key(side: &str) -> Option<SnapSide> {
    match side {
        "left" => Some(SnapSide::Left),
        "right" => Some(SnapSide::Right),
        _ => None,
    }
}

pub fn remember_snap_for_current_monitor(app: &AppHandle) {
    let Some(snap) = should_snap_to_edge(app) else {
        return;
    };
    let Some(bounds) = get_pet_bounds(app) else {
        return;
    };
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    let placement = prefs
        .monitor_positions
        .entry(snap.monitor.key.clone())
        .or_default();
    placement.x = bounds.x;
    placement.y = bounds.y;
    placement.mini_side = Some(snap_side_key(snap.side).into());
    prefs::save(app, &prefs);
}

fn preferred_snap_side(
    monitor: &MonitorArea,
    bounds: &WindowBounds,
    prefs: &prefs::Prefs,
) -> SnapSide {
    if let Some(saved) = prefs
        .monitor_positions
        .get(&monitor.key)
        .and_then(|placement| placement.mini_side.as_deref())
        .and_then(snap_side_from_key)
    {
        return saved;
    }

    let monitor_mid = monitor.x + monitor.width as i32 / 2;
    let pet_mid = bounds.x + bounds.width as i32 / 2;
    if pet_mid >= monitor_mid {
        SnapSide::Right
    } else {
        SnapSide::Left
    }
}

/// Exit mini mode: restore position, emit idle state, sync hit window.
/// Returns true if the pet was in mini mode and was restored.
pub fn do_exit_mini(app: &AppHandle) -> bool {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return false;
    };
    let monitor_key = get_pet_bounds(app)
        .and_then(|bounds| windows::monitor_for_bounds(app, &bounds))
        .map(|monitor| monitor.key);
    let (was_mini, restore_x, restore_y) = {
        let mut p = prefs_state.lock_or_recover();
        if !p.mini_mode {
            return false;
        }
        p.mini_mode = false;
        let restore = monitor_key
            .as_ref()
            .and_then(|key| p.monitor_positions.get(key))
            .map(|placement| (placement.x, placement.y))
            .unwrap_or((p.pre_mini_x, p.pre_mini_y));
        prefs::save(app, &p);
        (true, restore.0, restore.1)
    };
    if !was_mini {
        return false;
    }
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.set_position(PhysicalPosition::new(restore_x, restore_y));
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
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return false;
    };
    let bounds = match get_pet_bounds(app) {
        Some(bounds) => bounds,
        None => return false,
    };
    let monitor = match windows::monitor_for_bounds(app, &bounds) {
        Some(monitor) => monitor,
        None => return false,
    };

    // Determine target X: from edge snap (left or right), or default to right edge
    let (hidden_x, side) = if let Some(snap) = edge_snap_for_bounds(app, &bounds) {
        (snap.hidden_x(), snap.side)
    } else {
        let side = {
            let prefs = prefs_state.lock_or_recover();
            preferred_snap_side(&monitor, &bounds, &prefs)
        };
        let snap = EdgeSnap {
            monitor: monitor.clone(),
            width: bounds.width,
            side,
        };
        (snap.hidden_x(), side)
    };

    {
        let mut p = prefs_state.lock_or_recover();
        p.pre_mini_x = bounds.x;
        p.pre_mini_y = bounds.y;
        p.mini_mode = true;
        let placement = p.monitor_positions.entry(monitor.key.clone()).or_default();
        placement.x = bounds.x;
        placement.y = bounds.y;
        placement.mini_side = Some(snap_side_key(side).into());
        prefs::save(app, &p);
    }

    emit_state(app, "mini-idle", "clyde-mini-enter.svg");
    // animate_to_x automatically syncs hit window when animation completes
    animate_to_x(app, hidden_x, 300);
    true
}

/// Animate window with parabolic arc (jump transition).
#[allow(dead_code)]
pub fn animate_parabola(
    app: &AppHandle,
    target_x: i32,
    target_y: i32,
    peak_height: i32,
    duration_ms: u64,
) {
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
            SnapSide::Left => snap.hidden_x() + PEEK_OFFSET,
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
