use crate::state_machine::SharedState;
use crate::util::MutexExt;
use crate::windows::{self, compute_hit_rect, get_pet_bounds, HitBox};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK_INTERVAL_MS: u64 = 50;
const MOUSE_SLEEP_MS: u64 = 60_000;

/// Peek animation duration + margin — used to block re-peek during retraction.
const PEEK_ANIMATION_MS: u64 = 250;

/// Three-phase peek state machine to prevent oscillation.
/// Hidden → Peeking (mouse enters) → Retracting (mouse leaves) → Hidden.
/// While Retracting, new peek_in is blocked until the animation completes.
#[derive(Debug, Clone)]
pub enum PeekPhase {
    /// Pet is hidden at edge. Can transition to Peeking if mouse is near.
    Hidden,
    /// Pet is peeked out. Can transition to Retracting if mouse leaves.
    Peeking,
    /// Pet is animating back to hidden position. No new peek_in until done.
    Retracting(Instant),
}

#[derive(Debug, Clone)]
pub struct TickState {
    pub mouse_still_since: Instant,
    pub has_triggered_yawn: bool,
    pub has_triggered_wake: bool,
    pub last_eye_dx: f64,
    pub last_eye_dy: f64,
    pub peek_phase: PeekPhase,
}

impl Default for TickState {
    fn default() -> Self {
        TickState {
            mouse_still_since: Instant::now(),
            has_triggered_yawn: false,
            has_triggered_wake: false,
            last_eye_dx: 0.0,
            last_eye_dy: 0.0,
            peek_phase: PeekPhase::Hidden,
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

            // Mini mode hover peek: three-phase state machine to prevent oscillation.
            // Hidden → Peeking → Retracting → Hidden.  While Retracting, peek_in
            // is blocked until the retraction animation finishes.
            {
                let is_mini = crate::prefs::is_mini_mode(&app);
                if is_mini && !crate::mini::is_peek_suppressed(&app) {
                    if let Some(bounds) = get_pet_bounds(&app) {
                        // Use only the on-screen (visible) portion for near detection
                        // so the detection zone matches what the user actually sees.
                        let monitor = windows::monitor_for_bounds(&app, &bounds);
                        let (vis_x, vis_w) = if let Some(ref m) = monitor {
                            let left = bounds.x.max(m.x);
                            let right = (bounds.x + bounds.width as i32)
                                .min(m.x + m.width as i32);
                            (left, (right - left).max(0))
                        } else {
                            (bounds.x, bounds.width as i32)
                        };
                        let margin = (10.0 * windows::pet_scale_factor(&app)).round() as i32;
                        let near = cx >= (vis_x - margin) as f64
                            && cx <= (vis_x + vis_w + margin) as f64
                            && cy >= bounds.y as f64
                            && cy <= (bounds.y + bounds.height as i32) as f64;

                        let mut ts = tick_clone.lock_or_recover();
                        match ts.peek_phase {
                            PeekPhase::Hidden => {
                                if near {
                                    ts.peek_phase = PeekPhase::Peeking;
                                    drop(ts);
                                    let _ = app.emit("mini-peek-in", ());
                                }
                            }
                            PeekPhase::Peeking => {
                                if !near {
                                    ts.peek_phase =
                                        PeekPhase::Retracting(Instant::now());
                                    drop(ts);
                                    let _ = app.emit("mini-peek-out", ());
                                }
                            }
                            PeekPhase::Retracting(started) => {
                                let done = started.elapsed()
                                    >= Duration::from_millis(PEEK_ANIMATION_MS);
                                if done && !near {
                                    // Animation finished, mouse is away → fully hidden
                                    ts.peek_phase = PeekPhase::Hidden;
                                } else if done && near {
                                    // Animation finished but mouse came back → re-peek
                                    ts.peek_phase = PeekPhase::Peeking;
                                    drop(ts);
                                    let _ = app.emit("mini-peek-in", ());
                                }
                                // If !done: stay Retracting, block all transitions
                            }
                        }
                    }
                } else {
                    tick_clone.lock_or_recover().peek_phase = PeekPhase::Hidden;
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
