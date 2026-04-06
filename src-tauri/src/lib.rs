mod claude_monitor;
mod codex_monitor;
mod environment;
mod focus;
mod hooks;
mod http_server;
mod i18n;
mod mini;
mod permission;
mod permission_mode;
mod macos_spaces;
mod prefs;
mod session_meta;
mod state_machine;
mod tasks;
mod tick;
mod tray;
mod util;
mod windows;

use http_server::{ApprovalQueue, PendingPerms};
use prefs::SharedPrefs;
use serde::Serialize;
use state_machine::{SharedState, StateMachine};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use util::MutexExt;

// Animation duration constants (milliseconds)
const YAWN_DURATION_MS: u64 = 3000;
const DOZE_DURATION_MS: u64 = 4000;
const COLLAPSE_DURATION_MS: u64 = 3000;
const WAKE_DURATION_MS: u64 = 1500;
const MINI_IDLE_DELAY_MS: u64 = 500;
/// Shared task handle for sleep sequence, wake animation, and mini-enter delayed tasks.
/// Any new sleep/wake/mini transition should cancel the previous one.
pub type SleepAbortHandle = Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>;

/// Whether the pet is hidden to system tray.
pub type HiddenFlag = Arc<Mutex<bool>>;

#[derive(Clone, Serialize)]
struct PetConfig {
    opacity: f32,
}

#[derive(Clone, Serialize)]
struct InteractionState {
    position_locked: bool,
    click_through: bool,
}

#[derive(Serialize)]
struct MenuSession {
    id: String,
    state: String,
    agent: String,
    summary: String,
    project: String,
    short_id: String,
    pid: Option<u32>,
    updated_secs_ago: u64,
}

#[derive(Serialize)]
struct MenuData {
    sessions: Vec<MenuSession>,
    is_dnd: bool,
    is_mini: bool,
    lang: String,
    size: String,
    opacity: u8,
    permission_decision_window_secs: u16,
    position_locked: bool,
    click_through: bool,
    auto_hide_fullscreen: bool,
    auto_dnd_meetings: bool,
    auto_start_with_claude: bool,
    environment_controls_supported: bool,
}

struct DragState {
    active: bool,
    dragging: bool, // true once drag threshold is exceeded
    start_win_x: i32,
    start_win_y: i32,
    start_mouse_x: f64,
    start_mouse_y: f64,
}
type SharedDrag = Arc<Mutex<DragState>>;

/// Minimum mouse distance (physical pixels) before a drag actually starts.
const DRAG_THRESHOLD: f64 = 3.0;
const STARTUP_MIN_VISIBLE: i32 = 120;
const DISPLAY_REPAIR_DELAY_MS: u64 = 120;

fn emit_snap_preview(app: &AppHandle, side: Option<mini::SnapSide>) {
    let side = side.map(|side| match side {
        mini::SnapSide::Left => "left",
        mini::SnapSide::Right => "right",
    });
    let _ = app.emit(
        "snap-preview",
        serde_json::json!({
            "active": side.is_some(),
            "side": side,
        }),
    );
}

fn emit_pet_config(app: &AppHandle, prefs: &prefs::Prefs) {
    let _ = app.emit(
        "pet-config-changed",
        PetConfig {
            opacity: prefs.opacity,
        },
    );
}

fn emit_interaction_state(app: &AppHandle, prefs: &prefs::Prefs) {
    let _ = app.emit(
        "interaction-state-changed",
        InteractionState {
            position_locked: prefs.lock_position,
            click_through: prefs.click_through,
        },
    );
}

fn sync_autostart_pref(enabled: bool) {
    if let Err(e) = hooks::sync_auto_start_config(enabled) {
        eprintln!("Clyde: failed to sync auto-start config: {e}");
    }
}

pub(crate) fn format_relative_time(age_secs: u64, lang: &str) -> String {
    if age_secs < 10 {
        return i18n::t("sessionJustNow", lang);
    }
    if age_secs < 60 {
        return if lang == "zh" {
            format!("{age_secs}秒前")
        } else {
            format!("{age_secs}s ago")
        };
    }

    let minutes = age_secs / 60;
    if minutes < 60 {
        return if lang == "zh" {
            format!("{minutes}分钟前")
        } else {
            format!("{minutes}m ago")
        };
    }

    let hours = minutes / 60;
    if lang == "zh" {
        format!("{hours}小时前")
    } else {
        format!("{hours}h ago")
    }
}

pub(crate) fn platform_limited_menu_label(
    key: &str,
    lang: &str,
    checked: bool,
    enabled: bool,
) -> String {
    let label = if checked {
        format!("✓ {}", i18n::t(key, lang))
    } else {
        i18n::t(key, lang)
    };
    if enabled {
        label
    } else {
        format!("{label} ({})", i18n::t("macOnly", lang))
    }
}

fn apply_click_through(app: &AppHandle, enabled: bool) {
    let Some(hit) = app.get_webview_window("hit") else {
        return;
    };
    let _ = hit.set_ignore_cursor_events(enabled);
    if enabled {
        windows::hide_hit_window(app);
    } else {
        let is_auto_hidden = app
            .try_state::<SharedState>()
            .map(|state| state.lock_or_recover().auto_hidden)
            .unwrap_or(false);
        if is_auto_hidden {
            return;
        }
        if let Some(bounds) = windows::get_pet_bounds(app) {
            windows::sync_hit_window(app, &bounds, &windows::HitBox::INTERACTIVE);
        }
        windows::show_hit_window(app);
    }
}

fn persist_current_pet_position(app: &AppHandle) {
    let Some(bounds) = windows::get_pet_bounds(app) else {
        return;
    };
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    if prefs.mini_mode {
        return;
    }
    prefs.x = bounds.x;
    prefs.y = bounds.y;
    if let Some(monitor) = windows::monitor_for_bounds(app, &bounds) {
        let placement = prefs.monitor_positions.entry(monitor.key).or_default();
        placement.x = bounds.x;
        placement.y = bounds.y;
    }
    prefs::save(app, &prefs);
}

pub(crate) fn set_opacity(app: &AppHandle, opacity: f32) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.opacity = prefs::normalize_opacity(opacity);
    prefs::save(app, &prefs);
    emit_pet_config(app, &prefs);
}

pub(crate) fn set_permission_decision_window_secs(app: &AppHandle, secs: u16) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.permission_decision_window_secs = prefs::normalize_permission_decision_window_secs(secs);
    prefs::save(app, &prefs);
}

pub(crate) fn toggle_position_lock_pref(app: &AppHandle) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.lock_position = !prefs.lock_position;
    prefs::save(app, &prefs);
    emit_interaction_state(app, &prefs);
}

pub(crate) fn toggle_click_through_pref(app: &AppHandle) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.click_through = !prefs.click_through;
    let click_through = prefs.click_through;
    prefs::save(app, &prefs);
    emit_interaction_state(app, &prefs);
    drop(prefs);
    apply_click_through(app, click_through);
}

pub(crate) fn toggle_autostart_pref(app: &AppHandle) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.auto_start_with_claude = !prefs.auto_start_with_claude;
    let enabled = prefs.auto_start_with_claude;
    prefs::save(app, &prefs);
    drop(prefs);
    sync_autostart_pref(enabled);
}

pub(crate) fn toggle_auto_hide_fullscreen_pref(app: &AppHandle) {
    if !environment::controls_supported() {
        return;
    }
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.auto_hide_fullscreen = !prefs.auto_hide_fullscreen;
    prefs::save(app, &prefs);
}

pub(crate) fn toggle_auto_dnd_meetings_pref(app: &AppHandle) {
    if !environment::controls_supported() {
        return;
    }
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    prefs.auto_dnd_meetings = !prefs.auto_dnd_meetings;
    prefs::save(app, &prefs);
}


/// Returns the global mouse cursor position using CoreGraphics (macOS) or
/// falls back to the frontend-supplied coordinates on other platforms.
/// CG coordinates use a top-left origin that is DPI-independent, avoiding
/// the scale-factor mismatch that occurs when dragging across monitors with
/// different DPI settings.
#[cfg(target_os = "macos")]
fn native_cursor_position() -> Option<(f64, f64)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let pt = event.location();
    Some((pt.x, pt.y))
}

#[tauri::command]
fn drag_start(app: AppHandle, drag: tauri::State<SharedDrag>, x: f64, y: f64) {
    emit_snap_preview(&app, None);
    let mut d = drag.lock_or_recover();
    d.active = true;
    d.dragging = false;
    // On macOS, use native CG cursor position to avoid DPI mismatch across monitors.
    #[cfg(target_os = "macos")]
    let (mx, my) = native_cursor_position().unwrap_or((x, y));
    #[cfg(not(target_os = "macos"))]
    let (mx, my) = (x, y);
    d.start_mouse_x = mx;
    d.start_mouse_y = my;
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(pos) = pet.outer_position() {
            d.start_win_x = pos.x;
            d.start_win_y = pos.y;
        }
    }
}

#[tauri::command]
fn drag_move(app: AppHandle, drag: tauri::State<SharedDrag>, x: f64, y: f64) {
    let (active, dragging, base_x, base_y, smx, smy) = {
        let d = drag.lock_or_recover();
        (
            d.active,
            d.dragging,
            d.start_win_x,
            d.start_win_y,
            d.start_mouse_x,
            d.start_mouse_y,
        )
    };
    if !active {
        return;
    }
    let locked = app
        .try_state::<SharedPrefs>()
        .map(|prefs| prefs.lock_or_recover().lock_position)
        .unwrap_or(false);
    if locked {
        emit_snap_preview(&app, None);
        return;
    }

    // On macOS, use native CG cursor position to avoid DPI mismatch across monitors.
    #[cfg(target_os = "macos")]
    let (cx, cy) = native_cursor_position().unwrap_or((x, y));
    #[cfg(not(target_os = "macos"))]
    let (cx, cy) = (x, y);

    let dx = cx - smx;
    let dy = cy - smy;

    // Don't start moving until the mouse has moved past the drag threshold
    if !dragging {
        if (dx * dx + dy * dy).sqrt() < DRAG_THRESHOLD {
            return;
        }
        drag.lock_or_recover().dragging = true;
    }

    let mut new_x = base_x + dx as i32;
    let mut new_y = base_y + dy as i32;

    // Query pet bounds once for both clamp and hit-window sync
    let bounds = windows::get_pet_bounds(&app);
    let pet_w = bounds
        .as_ref()
        .map(|b| b.width)
        .unwrap_or(prefs::DEFAULT_PET_DIMENSION);
    let pet_h = bounds
        .as_ref()
        .map(|b| b.height)
        .unwrap_or(prefs::DEFAULT_PET_DIMENSION);
    let probe_bounds = windows::WindowBounds {
        x: new_x,
        y: new_y,
        width: pet_w,
        height: pet_h,
    };

    // Clamp to the union of all monitors so the pet can be dragged across displays
    // but still keeps at least 30px visible on some screen.
    const MIN_VISIBLE: i32 = 30;
    if let Some(union_rect) = windows::union_of_all_monitors(&app) {
        (new_x, new_y) =
            windows::clamp_window_to_monitor(new_x, new_y, pet_w, pet_h, &union_rect, MIN_VISIBLE);
    } else if let Some(monitor) = windows::monitor_for_bounds(&app, &probe_bounds)
        .or_else(|| windows::current_monitor_for_pet(&app))
    {
        (new_x, new_y) =
            windows::clamp_window_to_monitor(new_x, new_y, pet_w, pet_h, &monitor, MIN_VISIBLE);
    }

    // All coords are physical pixels — set_position directly
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.set_position(PhysicalPosition::new(new_x, new_y));
    }

    // Construct updated bounds from the new position + known size (avoid second IPC query)
    let updated = windows::WindowBounds {
        x: new_x,
        y: new_y,
        width: pet_w,
        height: pet_h,
    };
    windows::sync_hit_window(&app, &updated, &windows::HitBox::INTERACTIVE);
    emit_snap_preview(
        &app,
        mini::edge_snap_for_bounds(&app, &updated).map(|snap| snap.side),
    );
}

#[tauri::command]
fn drag_end(
    app: AppHandle,
    drag: tauri::State<SharedDrag>,
    abort_handle: tauri::State<'_, SleepAbortHandle>,
) {
    emit_snap_preview(&app, None);
    let was_dragging = {
        let mut d = drag.lock_or_recover();
        let dragging = d.dragging;
        d.active = false;
        d.dragging = false;
        dragging
    };

    let is_mini = prefs::is_mini_mode(&app);

    if is_mini {
        if !was_dragging {
            // Click in mini mode — just sync, don't change mode
            sync_hit(&app);
        } else if mini::should_snap_to_edge(&app).is_some() {
            // Dragged but still near edge — stay in mini mode
            mini::remember_snap_for_current_monitor(&app);
            sync_hit(&app);
        } else {
            // Dragged away from edge — exit mini mode
            cancel_pending_task(&abort_handle);
            mini::do_exit_mini(&app);
        }
    } else if was_dragging && mini::should_snap_to_edge(&app).is_some() {
        // Actually dragged to edge → enter mini mode (clicks won't trigger this).
        // animate_to_x inside do_enter_mini auto-syncs hit window on completion.
        if mini::do_enter_mini(&app) {
            cancel_pending_task(&abort_handle);
            let app2 = app.clone();
            let handle = tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(MINI_IDLE_DELAY_MS)).await;
                emit_state(&app2, "mini-idle", "clyde-mini-idle.svg");
            });
            *abort_handle.lock_or_recover() = Some(handle);
        }
    } else {
        // Normal click or drag end — sync hit window
        sync_hit(&app);
        if was_dragging {
            persist_current_pet_position(&app);
        }
    }
}

#[tauri::command]
fn exit_mini_mode(app: AppHandle, abort_handle: tauri::State<'_, SleepAbortHandle>) {
    cancel_pending_task(&abort_handle);
    mini::do_exit_mini(&app);
}

#[tauri::command]
fn hit_double_click(app: AppHandle, abort_handle: tauri::State<'_, SleepAbortHandle>) {
    let is_mini = prefs::is_mini_mode(&app);
    if is_mini {
        cancel_pending_task(&abort_handle);
        mini::do_exit_mini(&app);
        return;
    }
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.emit(
            "play-click-reaction",
            serde_json::json!({
                "svg": "clyde-react-double.svg", "duration_ms": 800
            }),
        );
    }
}

#[tauri::command]
fn hit_flail(app: AppHandle) {
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.emit(
            "play-click-reaction",
            serde_json::json!({
                "svg": "clyde-react-drag.svg", "duration_ms": 1200
            }),
        );
    }
}

#[tauri::command]
fn show_context_menu(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    prefs: tauri::State<SharedPrefs>,
) {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let (lang, is_mini, cur_size, cur_opacity, is_locked, click_through, auto_hide_fullscreen, auto_dnd_meetings, autostart, permission_decision_window_secs) = {
        let p = prefs.lock_or_recover();
        (
            p.lang.clone(),
            p.mini_mode,
            p.size.clone(),
            (p.opacity * 100.0).round() as i32,
            p.lock_position,
            p.click_through,
            p.auto_hide_fullscreen,
            p.auto_dnd_meetings,
            p.auto_start_with_claude,
            p.permission_decision_window_secs,
        )
    };
    let is_dnd = state.lock_or_recover().dnd;
    let environment_controls_supported = environment::controls_supported();

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();

    // Size submenu (with checkmark)
    if let (Ok(s), Ok(m), Ok(l)) = (
        MenuItem::with_id(
            &app,
            "ctx-size-s",
            if cur_size == "S" { "✓ S" } else { "S" },
            true,
            None::<&str>,
        ),
        MenuItem::with_id(
            &app,
            "ctx-size-m",
            if cur_size == "M" { "✓ M" } else { "M" },
            true,
            None::<&str>,
        ),
        MenuItem::with_id(
            &app,
            "ctx-size-l",
            if cur_size == "L" { "✓ L" } else { "L" },
            true,
            None::<&str>,
        ),
    ) {
        if let Ok(sub) = Submenu::with_items(&app, i18n::t("size", &lang), true, &[&s, &m, &l]) {
            items.push(Box::new(sub));
        }
    }

    let mut opacity_items = Vec::new();
    for level in [100, 90, 80, 70, 60, 50, 40] {
        let label = if cur_opacity == level {
            format!("✓ {level}%")
        } else {
            format!("{level}%")
        };
        if let Ok(item) = MenuItem::with_id(
            &app,
            format!("ctx-opacity-{level}"),
            label,
            true,
            None::<&str>,
        ) {
            opacity_items.push(item);
        }
    }
    let opacity_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = opacity_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    if let Ok(sub) = Submenu::with_items(&app, i18n::t("opacity", &lang), true, &opacity_refs) {
        items.push(Box::new(sub));
    }

    let mut permission_wait_items = Vec::new();
    for secs in [12_u16, 20, 30, 45, 60] {
        let label = if permission_decision_window_secs == secs {
            format!("✓ {secs}s")
        } else {
            format!("{secs}s")
        };
        if let Ok(item) = MenuItem::with_id(
            &app,
            format!("ctx-permission-timeout-{secs}"),
            label,
            true,
            None::<&str>,
        ) {
            permission_wait_items.push(item);
        }
    }
    let permission_wait_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = permission_wait_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    if let Ok(sub) = Submenu::with_items(
        &app,
        i18n::t("permissionWaitTime", &lang),
        true,
        &permission_wait_refs,
    ) {
        items.push(Box::new(sub));
    }

    // Mini mode
    let mini_label = if is_mini {
        format!("✓ {}", i18n::t("mini", &lang))
    } else {
        i18n::t("mini", &lang)
    };
    if let Ok(m) = MenuItem::with_id(&app, "ctx-mini", &mini_label, true, None::<&str>) {
        items.push(Box::new(m));
    }

    let lock_label = if is_locked {
        format!("✓ {}", i18n::t("lockPosition", &lang))
    } else {
        i18n::t("lockPosition", &lang)
    };
    if let Ok(m) = MenuItem::with_id(&app, "ctx-lock-position", &lock_label, true, None::<&str>) {
        items.push(Box::new(m));
    }

    let click_label = if click_through {
        format!("✓ {}", i18n::t("clickThrough", &lang))
    } else {
        i18n::t("clickThrough", &lang)
    };
    if let Ok(m) = MenuItem::with_id(&app, "ctx-click-through", &click_label, true, None::<&str>) {
        items.push(Box::new(m));
    }

    let fullscreen_label = platform_limited_menu_label(
        "hideOnFullscreen",
        &lang,
        auto_hide_fullscreen,
        environment_controls_supported,
    );
    if let Ok(m) = MenuItem::with_id(
        &app,
        "ctx-hide-on-fullscreen",
        &fullscreen_label,
        environment_controls_supported,
        None::<&str>,
    ) {
        items.push(Box::new(m));
    }

    let auto_dnd_label = platform_limited_menu_label(
        "autoDndMeetings",
        &lang,
        auto_dnd_meetings,
        environment_controls_supported,
    );
    if let Ok(m) = MenuItem::with_id(
        &app,
        "ctx-auto-dnd-meetings",
        &auto_dnd_label,
        environment_controls_supported,
        None::<&str>,
    ) {
        items.push(Box::new(m));
    }

    // DND
    let dnd_label = if is_dnd {
        format!("✓ {}", i18n::t("dnd", &lang))
    } else {
        i18n::t("dnd", &lang)
    };
    if let Ok(dnd) = MenuItem::with_id(&app, "ctx-dnd", &dnd_label, true, None::<&str>) {
        items.push(Box::new(dnd));
    }

    let autostart_label = if autostart {
        format!("✓ {}", i18n::t("autoStart", &lang))
    } else {
        i18n::t("autoStart", &lang)
    };
    if let Ok(item) = MenuItem::with_id(&app, "ctx-autostart", &autostart_label, true, None::<&str>)
    {
        items.push(Box::new(item));
    }

    if let Ok(sep) = PredefinedMenuItem::separator(&app) {
        items.push(Box::new(sep));
    }

    // Sessions submenu
    let sessions = state.lock_or_recover().session_summaries();
    let session_label = format!("{} ({})", i18n::t("sessions", &lang), sessions.len());
    let mut session_items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();
    if sessions.is_empty() {
        if let Ok(no) = MenuItem::with_id(
            &app,
            "ctx-none",
            i18n::t("noSessions", &lang),
            false,
            None::<&str>,
        ) {
            session_items.push(Box::new(no));
        }
    } else {
        for session in &sessions {
            let icon = match session.state.as_str() {
                "working" | "typing" => "⚡",
                "thinking" => "💭",
                "juggling" => "🎪",
                "idle" => "💤",
                "sleeping" => "😴",
                _ => "⚡",
            };
            let state_label = match session.state.as_str() {
                "working" | "typing" => i18n::t("sessionWorking", &lang),
                "thinking" => i18n::t("sessionThinking", &lang),
                "juggling" => i18n::t("sessionJuggling", &lang),
                "idle" => i18n::t("sessionIdle", &lang),
                "sleeping" => i18n::t("sessionSleeping", &lang),
                _ => session.state.clone(),
            };
            let display = session_meta::ensure_session_display_meta(
                state.inner(),
                &session.id,
                Some(session.agent_id.as_str()),
                if session.cwd.is_empty() {
                    None
                } else {
                    Some(session.cwd.as_str())
                },
            );
            let cached_summary = session.summary.trim();
            let title = if !cached_summary.is_empty() {
                cached_summary.to_string()
            } else if display.summary.is_empty() {
                format!("{} {}", display.agent_label, state_label)
            } else {
                display.summary.clone()
            };
            let mut meta_parts = vec![display.agent_label];
            if !display.project.is_empty() {
                meta_parts.push(display.project);
            }
            meta_parts.push(format_relative_time(session.updated_secs_ago, &lang));
            let label = format!("{icon} {title}  {}", meta_parts.join(" · "));
            let item_id = format!("ctx-session-{}", session.id);
            if let Ok(item) = MenuItem::with_id(&app, &item_id, &label, true, None::<&str>) {
                session_items.push(Box::new(item));
            }
        }
    }
    let sess_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        session_items.iter().map(|i| i.as_ref()).collect();
    if let Ok(sub) = Submenu::with_items(&app, &session_label, true, &sess_refs) {
        items.push(Box::new(sub));
    }

    if let Ok(sep) = PredefinedMenuItem::separator(&app) {
        items.push(Box::new(sep));
    }

    // Language submenu (with checkmark)
    let en_label = if lang == "en" {
        "✓ English"
    } else {
        "English"
    };
    let zh_label = if lang == "zh" { "✓ 中文" } else { "中文" };
    if let (Ok(en), Ok(zh)) = (
        MenuItem::with_id(&app, "ctx-lang-en", en_label, true, None::<&str>),
        MenuItem::with_id(&app, "ctx-lang-zh", zh_label, true, None::<&str>),
    ) {
        if let Ok(sub) = Submenu::with_items(&app, i18n::t("language", &lang), true, &[&en, &zh]) {
            items.push(Box::new(sub));
        }
    }

    // Tasks editor
    if let Ok(edit_tasks) = MenuItem::with_id(&app, "ctx-edit-tasks", "📋 编辑任务", true, None::<&str>) {
        items.push(Box::new(edit_tasks));
    }

    // Hide / About / Quit
    if let Ok(sep) = PredefinedMenuItem::separator(&app) {
        items.push(Box::new(sep));
    }
    if let Ok(hide) = MenuItem::with_id(
        &app,
        "ctx-hide",
        i18n::t("hide", &lang),
        true,
        None::<&str>,
    ) {
        items.push(Box::new(hide));
    }
    if let Ok(about) = MenuItem::with_id(
        &app,
        "ctx-about",
        i18n::t("about", &lang),
        true,
        None::<&str>,
    ) {
        items.push(Box::new(about));
    }
    if let Ok(q) = MenuItem::with_id(&app, "ctx-quit", i18n::t("quit", &lang), true, None::<&str>) {
        items.push(Box::new(q));
    }

    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        items.iter().map(|i| i.as_ref()).collect();
    if let Ok(menu) = Menu::with_items(&app, &item_refs) {
        if let Some(hit) = app.get_webview_window("hit") {
            let _ = hit.popup_menu(&menu);
        }
    }
}

#[tauri::command]
fn mini_peek_in(app: AppHandle) {
    emit_state(&app, "mini-peek", "clyde-mini-peek.svg");
    mini::peek_in(&app);
}

#[tauri::command]
fn mini_peek_out(app: AppHandle) {
    emit_state(&app, "mini-idle", "clyde-mini-idle.svg");
    mini::peek_out(&app);
}

#[tauri::command]
fn get_pet_config(prefs: tauri::State<'_, SharedPrefs>) -> PetConfig {
    let prefs = prefs.lock_or_recover();
    PetConfig {
        opacity: prefs.opacity,
    }
}

#[tauri::command]
fn get_interaction_state(prefs: tauri::State<'_, SharedPrefs>) -> InteractionState {
    let prefs = prefs.lock_or_recover();
    InteractionState {
        position_locked: prefs.lock_position,
        click_through: prefs.click_through,
    }
}

#[tauri::command]
fn get_menu_data(
    state: tauri::State<'_, SharedState>,
    prefs: tauri::State<'_, SharedPrefs>,
) -> MenuData {
    let prefs = prefs.lock_or_recover();
    let (sessions_raw, is_dnd) = {
        let sm = state.lock_or_recover();
        (sm.session_summaries(), sm.dnd)
    };
    let sessions = sessions_raw
        .into_iter()
        .map(|session| {
            let display = session_meta::ensure_session_display_meta(
                state.inner(),
                &session.id,
                Some(session.agent_id.as_str()),
                if session.cwd.is_empty() {
                    None
                } else {
                    Some(session.cwd.as_str())
                },
            );
            MenuSession {
                id: session.id,
                state: session.state,
                agent: display.agent_label,
                summary: display.summary,
                project: display.project,
                short_id: display.short_id,
                pid: session.source_pid,
                updated_secs_ago: session.updated_secs_ago,
            }
        })
        .collect();
    MenuData {
        sessions,
        is_dnd,
        is_mini: prefs.mini_mode,
        lang: prefs.lang.clone(),
        size: prefs.size.clone(),
        opacity: (prefs.opacity * 100.0).round() as u8,
        permission_decision_window_secs: prefs.permission_decision_window_secs,
        position_locked: prefs.lock_position,
        click_through: prefs.click_through,
        auto_hide_fullscreen: prefs.auto_hide_fullscreen,
        auto_dnd_meetings: prefs.auto_dnd_meetings,
        auto_start_with_claude: prefs.auto_start_with_claude,
        environment_controls_supported: environment::controls_supported(),
    }
}

#[tauri::command]
fn menu_action(app: AppHandle, state: tauri::State<'_, SharedState>, id: String) {
    handle_context_menu_event(&app, &state, &id);
}

/// Shared DND toggle — used from tauri command, context menu, and tray handler.
pub(crate) fn do_toggle_dnd(app: &AppHandle, state: &SharedState) {
    let new_dnd = {
        let mut sm = state.lock_or_recover();
        sm.toggle_manual_dnd();
        sm.dnd
    };
    let _ = app.emit("dnd-change", serde_json::json!({ "enabled": new_dnd }));
}

pub(crate) fn set_auto_dnd(app: &AppHandle, state: &SharedState, enabled: bool) {
    let changed = {
        let mut sm = state.lock_or_recover();
        if sm.auto_dnd == enabled {
            false
        } else {
            sm.set_auto_dnd(enabled)
        }
    };
    if !changed {
        return;
    }
    let new_dnd = state.lock_or_recover().dnd;
    let _ = app.emit("dnd-change", serde_json::json!({ "enabled": new_dnd }));
    if let Some(lang) = app
        .try_state::<SharedPrefs>()
        .map(|prefs| prefs.lock_or_recover().lang.clone())
    {
        tray::rebuild_menu(app, &lang);
    }
}

pub(crate) fn set_auto_hidden(app: &AppHandle, state: &SharedState, enabled: bool) {
    let changed = {
        let mut sm = state.lock_or_recover();
        if sm.auto_hidden == enabled {
            false
        } else {
            sm.auto_hidden = enabled;
            true
        }
    };
    if !changed {
        return;
    }

    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };
    if enabled {
        let _ = pet.hide();
        windows::hide_hit_window(app);
        return;
    }

    // Don't auto-restore if user manually hid to tray
    if is_hidden(app) {
        return;
    }

    let _ = pet.show();
    let click_through = app
        .try_state::<SharedPrefs>()
        .map(|prefs| prefs.lock_or_recover().click_through)
        .unwrap_or(false);
    apply_click_through(app, click_through);
}

#[tauri::command]
fn toggle_dnd(app: AppHandle, state: tauri::State<'_, SharedState>) {
    do_toggle_dnd(&app, &state);
}

/// Hide pet + hit + bubble windows to system tray.
pub(crate) fn do_hide_to_tray(app: &AppHandle) {
    if let Some(flag) = app.try_state::<HiddenFlag>() {
        *flag.lock_or_recover() = true;
    }
    // Hide bubble windows (don't destroy — keeps pending permission requests alive)
    if let Some(bubbles) = app.try_state::<permission::BubbleMap>() {
        permission::hide_all_bubbles(app, &bubbles);
    }
    if let Some(pet) = app.get_webview_window("pet") { let _ = pet.hide(); }
    if let Some(hit) = app.get_webview_window("hit") { let _ = hit.hide(); }
    // Rebuild tray menu to reflect new state
    let lang = app.try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().lang.clone())
        .unwrap_or_else(|| "en".into());
    tray::rebuild_menu(app, &lang);
}

/// Show pet + hit + bubble windows from system tray.
pub(crate) fn do_show_from_tray(app: &AppHandle) {
    if let Some(flag) = app.try_state::<HiddenFlag>() {
        *flag.lock_or_recover() = false;
    }
    if let Some(pet) = app.get_webview_window("pet") { let _ = pet.show(); }
    // Re-apply click-through state: if on, keep hit window hidden;
    // if off, show and sync the hit window.
    let click_through = app
        .try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().click_through)
        .unwrap_or(false);
    apply_click_through(app, click_through);
    // Restore bubble windows
    if let Some(bubbles) = app.try_state::<permission::BubbleMap>() {
        permission::show_all_bubbles(app, &bubbles);
    }
    let lang = app.try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().lang.clone())
        .unwrap_or_else(|| "en".into());
    tray::rebuild_menu(app, &lang);
}

/// Toggle visibility — used from tray click and context menu.
pub(crate) fn do_toggle_visibility(app: &AppHandle) {
    let hidden = app.try_state::<HiddenFlag>()
        .map(|f| *f.lock_or_recover())
        .unwrap_or(false);
    if hidden { do_show_from_tray(app); } else { do_hide_to_tray(app); }
}

pub(crate) fn is_hidden(app: &AppHandle) -> bool {
    app.try_state::<HiddenFlag>()
        .map(|f| *f.lock_or_recover())
        .unwrap_or(false)
}

pub(crate) fn emit_state(app: &AppHandle, state_str: &str, svg: &str) {
    let flip = is_left_mini(app);
    let is_mini = prefs::is_mini_mode(&app);

    // In mini mode, map normal states to mini SVGs so the pet shows real-time status
    let (out_state, out_svg) = if is_mini && !state_str.starts_with("mini-") {
        let mini = mini_svg_for_state(state_str);
        (mini.0, mini.1)
    } else {
        (state_str, svg)
    };

    let _ = app.emit(
        "state-change",
        serde_json::json!({ "state": out_state, "svg": out_svg, "flip": flip }),
    );
}

pub(crate) fn dismiss_transient_ui(
    app: &AppHandle,
    state: &SharedState,
    bubbles: &permission::BubbleMap,
) {
    let dismissed = {
        let mut sm = state.lock_or_recover();
        sm.dismiss_transient_state()
    };
    if let Some((resolved, svg)) = dismissed {
        emit_state(app, &resolved, &svg);
    }
    permission::close_mode_notice_bubbles(app, bubbles);
}

/// Map a normal state to its mini-mode equivalent.
fn mini_svg_for_state(state: &str) -> (&'static str, &'static str) {
    match state {
        "working" | "thinking" | "juggling" | "sweeping" | "carrying"
            => ("mini-alert", "clyde-mini-alert.svg"),
        "attention" | "notification"
            => ("mini-happy", "clyde-mini-happy.svg"),
        "error"
            => ("mini-alert", "clyde-mini-alert.svg"),
        "sleeping" | "yawning" | "dozing" | "collapsing"
            => ("mini-sleep", "clyde-mini-sleep.svg"),
        _ // idle, waking, etc.
            => ("mini-idle", "clyde-mini-idle.svg"),
    }
}

/// Check if currently in left-side mini mode.
fn is_left_mini(app: &AppHandle) -> bool {
    let is_mini = prefs::is_mini_mode(&app);
    if !is_mini {
        return false;
    }
    mini::should_snap_to_edge(app)
        .map(|s| s.side == mini::SnapSide::Left)
        .unwrap_or(false)
}

pub(crate) fn sync_hit(app: &AppHandle) {
    if let Some(bounds) = windows::get_pet_bounds(app) {
        sync_hit_for_bounds(app, &bounds);
    }
}

fn sync_hit_for_bounds(app: &AppHandle, bounds: &windows::WindowBounds) {
    windows::sync_hit_window(app, bounds, &windows::HitBox::INTERACTIVE);
}

fn preferred_bounds_for_current_display(
    app: &AppHandle,
    prefs: &prefs::Prefs,
) -> windows::WindowBounds {
    let (width, height) = prefs::size_to_pixels(&prefs.size);
    let mut x = prefs.x;
    let mut y = prefs.y;

    if let Some(monitors) = windows::available_monitor_areas(app) {
        if monitors.len() == 1 {
            if let Some(placement) = prefs.monitor_positions.get(&monitors[0].key) {
                x = placement.x;
                y = placement.y;
            }
        }
    }

    windows::WindowBounds {
        x,
        y,
        width,
        height,
    }
}

fn apply_pet_window_geometry(app: &AppHandle, bounds: &windows::WindowBounds) {
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    if let Some(current) = windows::get_pet_bounds(app) {
        if current.width != bounds.width || current.height != bounds.height {
            let _ = pet.set_size(tauri::PhysicalSize::new(bounds.width, bounds.height));
        }
        if current.x != bounds.x || current.y != bounds.y {
            let _ = pet.set_position(PhysicalPosition::new(bounds.x, bounds.y));
        }
    } else {
        let _ = pet.set_size(tauri::PhysicalSize::new(bounds.width, bounds.height));
        let _ = pet.set_position(PhysicalPosition::new(bounds.x, bounds.y));
    }
}

fn persist_pet_bounds(app: &AppHandle, bounds: &windows::WindowBounds) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };
    let mut prefs = prefs_state.lock_or_recover();
    if prefs.mini_mode {
        return;
    }
    prefs.x = bounds.x;
    prefs.y = bounds.y;
    if let Some(monitor) = windows::monitor_for_bounds(app, bounds) {
        let placement = prefs.monitor_positions.entry(monitor.key).or_default();
        placement.x = bounds.x;
        placement.y = bounds.y;
    }
    prefs::save(app, &prefs);
}

fn drag_in_progress(app: &AppHandle) -> bool {
    app.try_state::<SharedDrag>()
        .map(|drag| {
            let drag = drag.lock_or_recover();
            drag.active || drag.dragging
        })
        .unwrap_or(false)
}

fn should_restore_saved_single_monitor_position(
    current: &windows::WindowBounds,
    monitor: &windows::MonitorArea,
    saved: &prefs::MonitorPlacement,
) -> bool {
    let near_left = (current.x - monitor.x).abs() <= 36;
    let near_top = (current.y - monitor.y).abs() <= 36;
    let saved_far_x = (saved.x - current.x).abs() >= 80;
    let saved_far_y = (saved.y - current.y).abs() >= 80;
    (near_left || near_top) && (saved_far_x || saved_far_y)
}

fn reconcile_pet_geometry(app: &AppHandle) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        sync_hit(app);
        return;
    };
    let prefs_snapshot = prefs_state.lock_or_recover().clone();
    let mut target = preferred_bounds_for_current_display(app, &prefs_snapshot);

    if let Some(current_bounds) = windows::get_pet_bounds(app) {
        target.x = current_bounds.x;
        target.y = current_bounds.y;

        if let Some(monitors) = windows::available_monitor_areas(app) {
            if monitors.len() == 1 {
                if let Some(placement) = prefs_snapshot.monitor_positions.get(&monitors[0].key) {
                    if should_restore_saved_single_monitor_position(
                        &current_bounds,
                        &monitors[0],
                        placement,
                    ) {
                        target.x = placement.x;
                        target.y = placement.y;
                    }
                }
            }
        }
    } else if let Some(monitors) = windows::available_monitor_areas(app) {
        if monitors.len() == 1 {
            if let Some(placement) = prefs_snapshot.monitor_positions.get(&monitors[0].key) {
                target.x = placement.x;
                target.y = placement.y;
            }
        }
    }

    let (resolved_x, resolved_y) =
        windows::startup_position_for_bounds(app, &target, STARTUP_MIN_VISIBLE);
    let resolved = windows::WindowBounds {
        x: resolved_x,
        y: resolved_y,
        width: target.width,
        height: target.height,
    };

    apply_pet_window_geometry(app, &resolved);
    sync_hit_for_bounds(app, &resolved);
    persist_pet_bounds(app, &resolved);

    if let Some(bubbles) = app.try_state::<permission::BubbleMap>() {
        permission::reposition_bubbles(app, &bubbles);
    }
}

fn schedule_display_repair(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(DISPLAY_REPAIR_DELAY_MS)).await;
        if drag_in_progress(&app) {
            sync_hit(&app);
            return;
        }
        reconcile_pet_geometry(&app);
    });
}

/// Shared pipeline: lock state → update session → resolve → emit.
/// Available for monitors that don't need custom agent_id tagging.
#[allow(dead_code)]
pub(crate) fn update_session_and_emit(
    app: &AppHandle,
    state: &SharedState,
    session_id: &str,
    state_str: &str,
    event: &str,
) {
    let (resolved, svg) = {
        let mut sm = state.lock_or_recover();
        if event == "SessionEnd" {
            sm.handle_session_end(session_id);
        } else {
            sm.update_session_state(session_id, state_str, event);
        }
        let resolved = sm.resolve_display_state();
        let svg = sm.svg_for_state(&resolved);
        sm.current_state = resolved.clone();
        sm.current_svg = svg.clone();
        (resolved, svg)
    };
    emit_state(app, &resolved, &svg);
}

/// Atomically update state machine and emit to frontend.
fn transition(app: &AppHandle, state: &SharedState, state_str: &str, svg: &str) {
    {
        let mut sm = state.lock_or_recover();
        sm.current_state = state_str.into();
        sm.current_svg = svg.into();
    }
    emit_state(app, state_str, svg);
}

fn cancel_pending_task(handle: &SleepAbortHandle) {
    if let Some(old) = handle.lock_or_recover().take() {
        old.abort();
    }
}

#[tauri::command]
fn trigger_sleep_sequence(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    abort_handle: tauri::State<'_, SleepAbortHandle>,
) {
    cancel_pending_task(&abort_handle);

    transition(&app, &state, "yawning", "clyde-idle-yawn.svg");

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let handle = tauri::async_runtime::spawn(async move {
        // yawn → doze
        tokio::time::sleep(std::time::Duration::from_millis(YAWN_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "yawning" {
            return;
        }
        transition(&app2, &state2, "dozing", "clyde-idle-doze.svg");

        // doze → collapse
        tokio::time::sleep(std::time::Duration::from_millis(DOZE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "dozing" {
            return;
        }
        transition(&app2, &state2, "collapsing", "clyde-collapse-sleep.svg");

        // collapse → sleeping
        tokio::time::sleep(std::time::Duration::from_millis(COLLAPSE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "collapsing" {
            return;
        }
        transition(&app2, &state2, "sleeping", "clyde-sleeping.svg");
    });
    *abort_handle.lock_or_recover() = Some(handle);
}

#[tauri::command]
fn trigger_wake(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    abort_handle: tauri::State<'_, SleepAbortHandle>,
) {
    cancel_pending_task(&abort_handle);

    let current = state.lock_or_recover().current_state.clone();
    if !matches!(
        current.as_str(),
        "yawning" | "dozing" | "collapsing" | "sleeping"
    ) {
        return;
    }

    transition(&app, &state, "waking", "clyde-wake.svg");

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(WAKE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "waking" {
            return;
        }
        transition(&app2, &state2, "idle", "clyde-idle-follow.svg");
    });
    *abort_handle.lock_or_recover() = Some(handle);
}

#[tauri::command]
fn set_window_size(app: AppHandle, size: String, prefs: tauri::State<SharedPrefs>) {
    if let Some(_pet) = app.get_webview_window("pet") {
        // Capture position BEFORE resize (set_size may not update geometry instantly)
        let current_bounds = windows::get_pet_bounds(&app);
        let (w, h) = prefs::size_to_pixels(&size);
        let _ = _pet.set_size(tauri::PhysicalSize::new(w, h));
        if let Some(current) = current_bounds {
            let updated = windows::resized_pet_bounds(&current, w, h);
            windows::sync_hit_window(&app, &updated, &windows::HitBox::INTERACTIVE);
        }
        let mut p = prefs.lock_or_recover();
        p.size = size;
        prefs::save(&app, &p);
        tray::rebuild_menu(&app, &p.lang);
    }
}

#[tauri::command]
fn set_lang(app: AppHandle, lang: String, prefs: tauri::State<SharedPrefs>) {
    {
        let mut p = prefs.lock_or_recover();
        p.lang = lang.clone();
        prefs::save(&app, &p);
    }
    let _ = app.emit("lang-changed", &lang);
    tray::rebuild_menu(&app, &lang);
}

fn handle_context_menu_event(app: &AppHandle, state: &SharedState, id: &str) {
    // Session focus — support both "ctx-session-X" (tray) and "session-X" (custom menu)
    let session_id = id
        .strip_prefix("ctx-session-")
        .or_else(|| id.strip_prefix("session-"));
    if let Some(session_id) = session_id {
        let sm = state.lock_or_recover();
        if let Some(entry) = sm.sessions.get(session_id) {
            if let Some(pid) = entry.source_pid {
                let cwd = entry.cwd.clone();
                drop(sm);
                focus::focus_window_by_pid(pid, &cwd);
            }
        }
        return;
    }
    // Only handle context-menu items (ctx- prefix). Tray menu items
    // (no prefix) are handled by tray::handle_tray_event — ignore them
    // here to avoid double-toggling.
    let Some(action) = id.strip_prefix("ctx-") else {
        return;
    };
    let mut refresh_tray = false;
    match action {
        "dnd" => {
            do_toggle_dnd(app, state);
            refresh_tray = true;
        }
        "mini" => {
            if prefs::is_mini_mode(app) {
                mini::do_exit_mini(app);
            } else {
                mini::do_enter_mini(app);
            }
            refresh_tray = true;
        }
        "lock-position" => {
            toggle_position_lock_pref(app);
            refresh_tray = true;
        }
        "click-through" => {
            toggle_click_through_pref(app);
            refresh_tray = true;
        }
        "hide-on-fullscreen" => {
            toggle_auto_hide_fullscreen_pref(app);
            refresh_tray = true;
        }
        "auto-dnd-meetings" => {
            toggle_auto_dnd_meetings_pref(app);
            refresh_tray = true;
        }
        "autostart" => {
            toggle_autostart_pref(app);
            refresh_tray = true;
        }
        "size-s" => {
            tray::apply_size_pub(app, "S");
            refresh_tray = true;
        }
        "size-m" => {
            tray::apply_size_pub(app, "M");
            refresh_tray = true;
        }
        "size-l" => {
            tray::apply_size_pub(app, "L");
            refresh_tray = true;
        }
        "opacity-100" => {
            set_opacity(app, 1.0);
            refresh_tray = true;
        }
        "opacity-90" => {
            set_opacity(app, 0.9);
            refresh_tray = true;
        }
        "opacity-80" => {
            set_opacity(app, 0.8);
            refresh_tray = true;
        }
        "opacity-70" => {
            set_opacity(app, 0.7);
            refresh_tray = true;
        }
        "opacity-60" => {
            set_opacity(app, 0.6);
            refresh_tray = true;
        }
        "opacity-50" => {
            set_opacity(app, 0.5);
            refresh_tray = true;
        }
        "opacity-40" => {
            set_opacity(app, 0.4);
            refresh_tray = true;
        }
        "permission-timeout-12" => {
            set_permission_decision_window_secs(app, 12);
            refresh_tray = true;
        }
        "permission-timeout-20" => {
            set_permission_decision_window_secs(app, 20);
            refresh_tray = true;
        }
        "permission-timeout-30" => {
            set_permission_decision_window_secs(app, 30);
            refresh_tray = true;
        }
        "permission-timeout-45" => {
            set_permission_decision_window_secs(app, 45);
            refresh_tray = true;
        }
        "permission-timeout-60" => {
            set_permission_decision_window_secs(app, 60);
            refresh_tray = true;
        }
        "lang-en" => tray::apply_lang_pub(app, "en"),
        "lang-zh" => tray::apply_lang_pub(app, "zh"),
        "hide" => do_hide_to_tray(app),
        "about" => {
            let _ = open::that("https://github.com/QingJ01/Clyde");
        }
        "quit" => app.exit(0),
        "edit-tasks" => {
            show_tasks_editor(app);
        }
        _ => {}
    }
    if refresh_tray {
        if let Some(lang) = app
            .try_state::<SharedPrefs>()
            .map(|prefs| prefs.lock_or_recover().lang.clone())
        {
            tray::rebuild_menu(app, &lang);
        }
    }
}

fn show_tasks_editor(app: &AppHandle) {
    use tauri::WebviewWindowBuilder;
    // If already open, just focus it
    if let Some(win) = app.get_webview_window("tasks") {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    // Position near the pet window
    let (x, y) = if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(pos) = pet.outer_position() {
            (pos.x + 20, pos.y + 20)
        } else {
            (200, 200)
        }
    } else {
        (200, 200)
    };

    if let Ok(win) = WebviewWindowBuilder::new(
        app,
        "tasks",
        tauri::WebviewUrl::App("src/windows/tasks/index.html".into()),
    )
    .title("Tasks")
    .inner_size(240.0, 260.0)
    .position(x as f64, y as f64)
    .decorations(false)
    .transparent(false)
    .shadow(true)
    .always_on_top(true)
    .resizable(false)
    .skip_taskbar(true)
    .build()
    {
        // Use a dark, opaque background so macOS receives mouse events.
        // (Fully transparent windows on macOS don't receive clicks.)
        let _ = win.set_background_color(Some(tauri::window::Color(30, 30, 30, 255)));
    }
}

fn setup_pet_window(app: &AppHandle, prefs: &prefs::Prefs) -> Option<windows::WindowBounds> {
    let Some(pet) = app.get_webview_window("pet") else {
        eprintln!("Clyde: pet window not found!");
        return None;
    };
    let desired_bounds = preferred_bounds_for_current_display(app, prefs);
    let (w, h) = (desired_bounds.width, desired_bounds.height);
    let (resolved_x, resolved_y) =
        windows::startup_position_for_bounds(app, &desired_bounds, STARTUP_MIN_VISIBLE);
    let resolved_bounds = windows::WindowBounds {
        x: resolved_x,
        y: resolved_y,
        width: w,
        height: h,
    };

    if (resolved_x, resolved_y) != (prefs.x, prefs.y) {
        if let Some(shared_prefs) = app.try_state::<SharedPrefs>() {
            let mut saved = shared_prefs.lock_or_recover();
            saved.x = resolved_x;
            saved.y = resolved_y;
            if let Some(monitor) = windows::monitor_for_bounds(app, &resolved_bounds) {
                let placement = saved.monitor_positions.entry(monitor.key).or_default();
                placement.x = resolved_x;
                placement.y = resolved_y;
            }
            prefs::save(app, &saved);
        }
    }

    if let Err(e) = pet.set_background_color(Some(Color(0, 0, 0, 0))) {
        eprintln!("Clyde: set_background_color failed: {e}");
    }
    let _ = pet.set_ignore_cursor_events(true);
    apply_pet_window_geometry(app, &resolved_bounds);
    macos_spaces::apply_space_follow(&pet);
    if let Err(e) = pet.show() {
        eprintln!("Clyde: pet.show() failed: {e}");
    }
    // Re-apply overlay properties after show() to ensure they take effect in
    // release builds (Tauri may reset native window state during show).
    macos_spaces::apply_space_follow(&pet);
    emit_pet_config(app, prefs);
    println!(
        "Clyde: pet window shown ({}x{}) at ({},{})",
        w, h, resolved_x, resolved_y
    );
    #[cfg(debug_assertions)]
    pet.open_devtools();
    Some(resolved_bounds)
}

fn setup_hit_window(app: &AppHandle, initial_bounds: Option<&windows::WindowBounds>) {
    if let Some(hit) = app.get_webview_window("hit") {
        // macOS needs a near-transparent background (not fully transparent)
        // to receive pointer events. Windows/Linux work with fully transparent.
        #[cfg(target_os = "macos")]
        let _ = hit.set_background_color(Some(Color(0, 0, 0, 1)));
        #[cfg(not(target_os = "macos"))]
        let _ = hit.set_background_color(Some(Color(0, 0, 0, 0)));
    }
    let fallback_bounds = if initial_bounds.is_none() {
        windows::get_pet_bounds(app)
    } else {
        None
    };
    if let Some(bounds) = initial_bounds.or(fallback_bounds.as_ref()) {
        sync_hit_for_bounds(app, bounds);
    } else {
        eprintln!("Clyde: could not get pet bounds for hit window sync");
    }
    if let Some(hit) = app.get_webview_window("hit") {
        macos_spaces::apply_space_follow(&hit);
    }
    windows::show_hit_window(app);
    // Re-apply after show() for release build safety.
    if let Some(hit) = app.get_webview_window("hit") {
        macos_spaces::apply_space_follow(&hit);
    }
    let click_through = app
        .try_state::<SharedPrefs>()
        .map(|prefs| {
            let prefs = prefs.lock_or_recover();
            emit_interaction_state(app, &prefs);
            prefs.click_through
        })
        .unwrap_or(false);
    apply_click_through(app, click_through);
}

fn setup_tray(app: &AppHandle, prefs: &prefs::Prefs, shared_tray: &tray::SharedTray) {
    if prefs.show_tray {
        match tray::build_tray(app, &prefs.lang) {
            Ok(tray_icon) => {
                *shared_tray.lock_or_recover() = Some(tray_icon);
                println!("Clyde: tray icon created");
            }
            Err(e) => eprintln!("Clyde: tray error: {e}"),
        }
    }
}

#[cfg(target_os = "macos")]
fn setup_active_space_observer(app: &AppHandle, bubbles: permission::BubbleMap) {
    let app_for_space = app.clone();
    macos_spaces::install_active_space_observer(move || {
        let app_for_main = app_for_space.clone();
        let bubbles_for_main = bubbles.clone();
        let _ = app_for_space.run_on_main_thread(move || {
            if let Some(pet) = app_for_main.get_webview_window("pet") {
                macos_spaces::refresh_space_follow(&pet);
            }
            if let Some(hit) = app_for_main.get_webview_window("hit") {
                macos_spaces::refresh_space_follow(&hit);
            }

            let bubble_ids: Vec<String> = bubbles_for_main
                .lock_or_recover()
                .keys()
                .cloned()
                .collect();
            for id in bubble_ids {
                if let Some(win) = app_for_main.get_webview_window(&format!("bubble-{id}")) {
                    macos_spaces::refresh_space_follow(&win);
                }
            }

            sync_hit(&app_for_main);
            permission::reposition_bubbles(&app_for_main, &bubbles_for_main);
        });
    });
}

fn start_cleanup_loop(app: &AppHandle, state: SharedState) {
    let state_for_cleanup = state;
    let app_for_cleanup = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let changed = state_for_cleanup.lock_or_recover().clean_stale();
            if changed {
                let (resolved, svg) = {
                    let sm = state_for_cleanup.lock_or_recover();
                    let r = sm.resolve_display_state();
                    let s = sm.svg_for_state(&r);
                    (r, s)
                };
                // Lock dropped before emit_state to avoid holding state across prefs/mini locks
                emit_state(&app_for_cleanup, &resolved, &svg);
            }
        }
    });
}

pub fn run() {
    let drag_state: SharedDrag = Arc::new(Mutex::new(DragState {
        active: false,
        dragging: false,
        start_win_x: 0,
        start_win_y: 0,
        start_mouse_x: 0.0,
        start_mouse_y: 0.0,
    }));
    let shared_state: SharedState = Arc::new(Mutex::new(StateMachine::new()));
    let pending_perms: PendingPerms = Arc::new(Mutex::new(HashMap::new()));
    let approval_queue: ApprovalQueue =
        Arc::new(Mutex::new(http_server::ApprovalQueueState::default()));
    let shared_prefs: SharedPrefs = Arc::new(Mutex::new(prefs::Prefs::default()));
    let sleep_abort: SleepAbortHandle = Arc::new(Mutex::new(None));
    let bubble_map: permission::BubbleMap = Arc::new(Mutex::new(HashMap::new()));
    let mode_tracker: permission_mode::ModeTracker = Arc::new(Mutex::new(HashMap::new()));
    let shared_tray: tray::SharedTray = Arc::new(Mutex::new(None));
    let hidden_flag: HiddenFlag = Arc::new(Mutex::new(false));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .manage(drag_state)
        .manage(shared_state.clone())
        .manage(pending_perms.clone())
        .manage(approval_queue.clone())
        .manage(shared_prefs.clone())
        .manage(sleep_abort.clone())
        .manage(bubble_map.clone())
        .manage(mode_tracker.clone())
        .manage(shared_tray.clone())
        .manage(hidden_flag)
        .invoke_handler(tauri::generate_handler![
            drag_start, drag_move, drag_end, exit_mini_mode,
            hit_double_click, hit_flail, show_context_menu,
            toggle_dnd, mini_peek_in, mini_peek_out,
            get_pet_config, get_interaction_state, get_menu_data, menu_action,
            http_server::resolve_permission,
            trigger_sleep_sequence,
            trigger_wake,
            set_window_size,
            set_lang,
            permission::get_bubble_data,
            permission::bubble_height_measured,
            permission::dismiss_bubble,
            focus::focus_terminal_for_session,
            tasks::get_tasks,
            tasks::set_tasks,
            tasks::update_task,
            tasks::add_task,
            tasks::remove_task,
            tasks::reorder_tasks,
            tasks::close_tasks_editor,
        ])
        .setup(move |app| {
            // Hide Dock icon — the app is controlled via the tray icon instead.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let prefs = prefs::load(app.handle());
            *shared_prefs.lock_or_recover() = prefs.clone();
            sync_autostart_pref(prefs.auto_start_with_claude);

            let initial_bounds = setup_pet_window(app.handle(), &prefs);
            setup_hit_window(app.handle(), initial_bounds.as_ref());
            setup_tray(app.handle(), &prefs, &shared_tray);
            #[cfg(target_os = "macos")]
            setup_active_space_observer(app.handle(), bubble_map.clone());

            // Intercept close → hide to tray (save position first)
            if let Some(pet_win) = app.get_webview_window("pet") {
                let handle_for_close = app.handle().clone();
                pet_win.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            if let Some(bounds) = windows::get_pet_bounds(&handle_for_close) {
                                persist_pet_bounds(&handle_for_close, &bounds);
                            }
                            // Only hide to tray if tray icon exists; otherwise let close proceed (quit)
                            let tray_exists = handle_for_close.try_state::<tray::SharedTray>()
                                .map(|t| t.lock_or_recover().is_some())
                                .unwrap_or(false);
                            if tray_exists {
                                api.prevent_close();
                                do_hide_to_tray(&handle_for_close);
                            }
                        }
                        tauri::WindowEvent::Moved(_)
                        | tauri::WindowEvent::Resized(_)
                        | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                            schedule_display_repair(handle_for_close.clone());
                        }
                        _ => {}
                    }
                });
            }

            // Start HTTP server + register hooks
            {
                let handle = app.handle().clone();
                let state_clone = shared_state.clone();
                let perms_clone = pending_perms.clone();
                let approval_queue_clone = approval_queue.clone();
                let bubbles_clone = bubble_map.clone();
                let mode_clone = mode_tracker.clone();
                let auto_start_enabled = prefs.auto_start_with_claude;
                tauri::async_runtime::spawn(async move {
                    match http_server::start_server(
                        handle.clone(),
                        state_clone,
                        perms_clone,
                        approval_queue_clone,
                        bubbles_clone,
                        mode_clone,
                    )
                    .await
                    {
                        Some(port) => {
                            let installer = hooks::HookInstaller {
                                settings_path: None,
                                server_port: Some(port),
                                auto_start_enabled,
                            };
                            if let Err(e) = installer.register() {
                                eprintln!("Clyde: failed to register hooks: {e}");
                            } else {
                                // Verify permission hook health after registration
                                let perm_url = format!("http://127.0.0.1:{port}/permission");
                                if let Some(settings_path) = dirs::home_dir()
                                    .map(|h| h.join(".claude").join("settings.json"))
                                {
                                    if let Ok(raw) = std::fs::read_to_string(&settings_path) {
                                        if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&raw) {
                                            if hooks::permission_hook_is_healthy(&settings, &perm_url) {
                                                println!("Clyde: permission hook verified — {perm_url}");
                                            } else {
                                                eprintln!("Clyde: WARNING — permission hook may be malformed in {}", settings_path.display());
                                                eprintln!("Clyde: expected nested format with URL {perm_url}");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            eprintln!("Clyde: HTTP server failed to start — skipping hook installation");
                        }
                    }
                });
            }

            // Context menu event handling on hit window
            {
                let app_for_menu = app.handle().clone();
                let state_for_menu = shared_state.clone();
                if let Some(hit) = app.get_webview_window("hit") {
                    hit.on_menu_event(move |_win, event| {
                        handle_context_menu_event(&app_for_menu, &state_for_menu, event.id().as_ref());
                    });
                }
            }

            tick::start_tick(app.handle().clone(), shared_state.clone());
            codex_monitor::start_codex_monitor(app.handle().clone(), shared_state.clone());
            claude_monitor::start_claude_monitor(app.handle().clone(), shared_state.clone());
            start_cleanup_loop(app.handle(), shared_state.clone());
            environment::start_environment_loop(app.handle(), shared_state.clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
