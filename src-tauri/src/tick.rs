use crate::state_machine::SharedState;
use crate::util::MutexExt;
use crate::windows::{self, compute_hit_rect, get_pet_bounds, HitBox};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK_INTERVAL_MS: u64 = 50;
const MOUSE_SLEEP_MS: u64 = 60_000;

#[derive(Debug, Clone)]
pub struct TickState {
    pub mouse_still_since: Instant,
    pub has_triggered_yawn: bool,
    pub has_triggered_wake: bool,
    pub last_eye_dx: f64,
    pub last_eye_dy: f64,
    pub is_peeking: bool,
    /// When peek_out was last triggered — used to prevent peek_in from
    /// re-triggering while the retraction animation is still running.
    pub peek_out_at: Option<Instant>,
}

impl Default for TickState {
    fn default() -> Self {
        TickState {
            mouse_still_since: Instant::now(),
            has_triggered_yawn: false,
            has_triggered_wake: false,
            last_eye_dx: 0.0,
            last_eye_dy: 0.0,
            is_peeking: false,
            peek_out_at: None,
        }
    }
}

pub type SharedTickState = Arc<Mutex<TickState>>;

#[derive(Clone, Serialize)]
struct EyeMovePayload {
    dx: f64,
    dy: f64,
}

/// Reads current_state directly from SharedState (lock hold time: one String clone).
/// This eliminates the need for a separate 200ms sync task + extra Arc<Mutex<String>>.
pub fn start_tick(app: AppHandle, state: SharedState) -> SharedTickState {
    let tick_state: SharedTickState = Arc::new(Mutex::new(TickState::default()));
    let tick_clone = tick_state.clone();

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TICK_INTERVAL_MS));
        let mut last_x: f64 = -1.0;
        let mut last_y: f64 = -1.0;

        loop {
            interval.tick().await;

            let cursor = match app.cursor_position() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let cx = cursor.x;
            let cy = cursor.y;

            let moved = (cx - last_x).abs() > 0.5 || (cy - last_y).abs() > 0.5;
            if moved {
                last_x = cx;
                last_y = cy;
            }

            let state_str = state.lock_or_recover().current_state.clone();
            let is_sleep_state = matches!(
                state_str.as_str(),
                "yawning" | "dozing" | "collapsing" | "sleeping"
            );

            // Single lock acquisition per tick for TickState
            let (should_yawn, should_wake) = {
                let mut ts = tick_clone.lock_or_recover();
                if moved {
                    ts.mouse_still_since = Instant::now();
                    ts.has_triggered_yawn = false;
                }
                // Reset wake flag when no longer in a sleep state
                if !is_sleep_state {
                    ts.has_triggered_wake = false;
                }
                let idle = ts.mouse_still_since.elapsed().as_millis() as u64;
                let yawn = state_str == "idle" && !ts.has_triggered_yawn && idle >= MOUSE_SLEEP_MS;
                let wake = moved && is_sleep_state && !ts.has_triggered_wake;
                if yawn {
                    ts.has_triggered_yawn = true;
                }
                if wake {
                    ts.has_triggered_wake = true;
                }
                (yawn, wake)
            };

            if should_wake {
                let _ = app.emit("trigger-wake", ());
            }
            if should_yawn {
                let _ = app.emit("trigger-yawn", ());
            }

            // Mini mode hover peek: detect mouse near pet in mini mode.
            // Single lock acquisition for all peek state reads/writes.
            {
                let is_mini = crate::prefs::is_mini_mode(&app);
                if is_mini {
                    if let Some(bounds) = get_pet_bounds(&app) {
                        // Symmetric 10 logical-px margin around pet for peek detection
                        let margin = (10.0 * windows::pet_scale_factor(&app)).round() as i32;
                        let near = cx >= (bounds.x - margin) as f64
                            && cx <= (bounds.x + bounds.width as i32 + margin) as f64
                            && cy >= bounds.y as f64
                            && cy <= (bounds.y + bounds.height as i32) as f64;
                        let mut ts = tick_clone.lock_or_recover();
                        let was_peeking = ts.is_peeking;
                        // Cooldown: don't re-trigger peek_in while the peek_out
                        // animation is still running (200ms + 50ms margin).
                        let in_cooldown = ts
                            .peek_out_at
                            .map_or(false, |t| t.elapsed() < Duration::from_millis(250));
                        if near && !was_peeking && !in_cooldown {
                            ts.is_peeking = true;
                            ts.peek_out_at = None;
                            drop(ts);
                            let _ = app.emit("mini-peek-in", ());
                        } else if !near && was_peeking {
                            ts.is_peeking = false;
                            ts.peek_out_at = Some(Instant::now());
                            drop(ts);
                            let _ = app.emit("mini-peek-out", ());
                        }
                    }
                } else {
                    tick_clone.lock_or_recover().is_peeking = false;
                }
            }

            // Eye tracking: only in idle state
            if state_str == "idle" {
                if let Some(bounds) = get_pet_bounds(&app) {
                    let rect = compute_hit_rect(&bounds, &HitBox::DEFAULT);
                    let center_x = (rect.left + rect.right) / 2.0;
                    let center_y = (rect.top + rect.bottom) / 2.0;
                    // Convert physical-pixel deltas to logical so the
                    // directional normalization is DPI-independent.
                    let scale = windows::pet_scale_factor(&app);
                    let raw_dx = (cx - center_x) / scale;
                    let raw_dy = (cy - center_y) / scale;
                    let dist = (raw_dx * raw_dx + raw_dy * raw_dy).sqrt().max(1.0);
                    let dx = (raw_dx / dist * 3.0).clamp(-3.0, 3.0);
                    let dy = (raw_dy / dist * 3.0).clamp(-3.0, 3.0);

                    let should_emit = {
                        let mut ts = tick_clone.lock_or_recover();
                        if (dx - ts.last_eye_dx).abs() > 0.1 || (dy - ts.last_eye_dy).abs() > 0.1 {
                            ts.last_eye_dx = dx;
                            ts.last_eye_dy = dy;
                            true
                        } else {
                            false
                        }
                    };
                    if should_emit {
                        if let Some(pet) = app.get_webview_window("pet") {
                            let _ = pet.emit("eye-move", EyeMovePayload { dx, dy });
                        }
                    }
                }
            }
        }
    });

    tick_state
}
