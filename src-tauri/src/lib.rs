mod windows;
mod tick;
mod state_machine;
mod http_server;
mod hooks;
mod i18n;
mod prefs;
mod tray;
mod mini;
mod codex_monitor;
mod claude_monitor;
mod permission;
mod permission_mode;
mod focus;
mod util;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use tauri::window::Color;
use state_machine::{SharedState, StateMachine};
use http_server::PendingPerms;
use prefs::SharedPrefs;
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

struct DragState {
    active:        bool,
    dragging:      bool,   // true once drag threshold is exceeded
    start_win_x:   i32,
    start_win_y:   i32,
    start_mouse_x: f64,
    start_mouse_y: f64,
}
type SharedDrag = Arc<Mutex<DragState>>;

/// Minimum mouse distance (physical pixels) before a drag actually starts.
const DRAG_THRESHOLD: f64 = 3.0;

#[tauri::command]
fn drag_start(app: AppHandle, drag: tauri::State<SharedDrag>, x: f64, y: f64) {
    let mut d = drag.lock_or_recover();
    d.active        = true;
    d.dragging      = false;
    // Frontend sends logical screenX/Y — store as-is (logical coords)
    d.start_mouse_x = x;
    d.start_mouse_y = y;
    // Window position in logical coords to match mouse coords
    if let Some(pet) = app.get_webview_window("pet") {
        if let Ok(pos) = pet.outer_position() {
            let scale = pet.scale_factor().unwrap_or(1.0);
            d.start_win_x = (pos.x as f64 / scale).round() as i32;
            d.start_win_y = (pos.y as f64 / scale).round() as i32;
        }
    }
}

#[tauri::command]
fn drag_move(app: AppHandle, drag: tauri::State<SharedDrag>, x: f64, y: f64) {
    let (active, dragging, base_x, base_y, smx, smy) = {
        let d = drag.lock_or_recover();
        (d.active, d.dragging, d.start_win_x, d.start_win_y, d.start_mouse_x, d.start_mouse_y)
    };
    if !active { return; }

    let dx = x - smx;
    let dy = y - smy;

    // Don't start moving until the mouse has moved past the drag threshold
    if !dragging {
        if (dx * dx + dy * dy).sqrt() < DRAG_THRESHOLD { return; }
        drag.lock_or_recover().dragging = true;
    }

    let mut new_x = base_x + dx as i32;
    let mut new_y = base_y + dy as i32;

    // Drag math uses logical pixels (matching frontend screenX/screenY).
    // get_pet_monitor() returns physical — convert to logical for clamp.
    let mon_phys = windows::get_pet_monitor(&app);
    let scale = app.get_webview_window("pet")
        .and_then(|p| p.scale_factor().ok()).unwrap_or(1.0);
    let mon_x = (mon_phys.x as f64 / scale).round() as i32;
    let mon_y = (mon_phys.y as f64 / scale).round() as i32;
    let mon_w = (mon_phys.width as f64 / scale).round() as i32;
    let mon_h = (mon_phys.height as f64 / scale).round() as i32;
    let pet_size = app.get_webview_window("pet")
        .and_then(|p| p.outer_size().ok()).unwrap_or_default();
    let pet_w_log = (pet_size.width as f64 / scale).round() as i32;

    const MIN_VISIBLE: i32 = 30;
    new_x = new_x.max(mon_x + MIN_VISIBLE - pet_w_log).min(mon_x + mon_w - MIN_VISIBLE);
    new_y = new_y.max(mon_y).min(mon_y + mon_h - MIN_VISIBLE);

    // Convert logical → physical for set_position
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.set_position(PhysicalPosition::new(
            (new_x as f64 * scale).round() as i32,
            (new_y as f64 * scale).round() as i32,
        ));
    }

    // Snap preview (logical coords)
    let near_edge = {
        let mon_right = mon_x + mon_w;
        let pet_right = new_x + pet_w_log;
        (mon_right - pet_right <= mini::SNAP_TOLERANCE) || (new_x - mon_x <= mini::SNAP_TOLERANCE)
    };
    let _ = app.emit("snap-preview", serde_json::json!({ "active": near_edge }));

    // Sync hit window with physical coordinates
    if let Some(bounds) = windows::get_pet_bounds(&app) {
        windows::sync_hit_window(&app, &bounds, &windows::HitBox::INTERACTIVE);
    }
}

#[tauri::command]
fn drag_end(app: AppHandle, drag: tauri::State<SharedDrag>, abort_handle: tauri::State<'_, SleepAbortHandle>) {
    // Clear snap preview
    let _ = app.emit("snap-preview", serde_json::json!({ "active": false }));

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
    }

    // Persist position after every drag so it survives crashes/force-quit
    if was_dragging {
        if let (Some(bounds), Some(prefs_state)) = (windows::get_pet_bounds(&app), app.try_state::<SharedPrefs>()) {
            let mut p = prefs_state.lock_or_recover();
            p.x = bounds.x;
            p.y = bounds.y;
            prefs::save(&app, &p);
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
        let _ = pet.emit("play-click-reaction", serde_json::json!({
            "svg": "clyde-react-double.svg", "duration_ms": 800
        }));
    }
}

#[tauri::command]
fn hit_flail(app: AppHandle) {
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.emit("play-click-reaction", serde_json::json!({
            "svg": "clyde-react-drag.svg", "duration_ms": 1200
        }));
    }
}

#[tauri::command]
fn show_context_menu(app: AppHandle, state: tauri::State<'_, SharedState>, prefs: tauri::State<SharedPrefs>) {
    use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem};

    let (lang, is_mini, cur_size) = {
        let p = prefs.lock_or_recover();
        (p.lang.clone(), p.mini_mode, p.size.clone())
    };
    let is_dnd = state.lock_or_recover().dnd;

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();

    // Size submenu (with checkmark)
    if let (Ok(s), Ok(m), Ok(l)) = (
        MenuItem::with_id(&app, "ctx-size-s", if cur_size == "S" { "✓ S" } else { "S" }, true, None::<&str>),
        MenuItem::with_id(&app, "ctx-size-m", if cur_size == "M" { "✓ M" } else { "M" }, true, None::<&str>),
        MenuItem::with_id(&app, "ctx-size-l", if cur_size == "L" { "✓ L" } else { "L" }, true, None::<&str>),
    ) {
        if let Ok(sub) = Submenu::with_items(&app, i18n::t("size", &lang), true, &[&s, &m, &l]) {
            items.push(Box::new(sub));
        }
    }

    // Mini mode
    let mini_label = if is_mini { format!("✓ {}", i18n::t("mini", &lang)) } else { i18n::t("mini", &lang) };
    if let Ok(m) = MenuItem::with_id(&app, "ctx-mini", &mini_label, true, None::<&str>) {
        items.push(Box::new(m));
    }

    // DND
    let dnd_label = if is_dnd { format!("✓ {}", i18n::t("dnd", &lang)) } else { i18n::t("dnd", &lang) };
    if let Ok(dnd) = MenuItem::with_id(&app, "ctx-dnd", &dnd_label, true, None::<&str>) {
        items.push(Box::new(dnd));
    }

    if let Ok(sep) = PredefinedMenuItem::separator(&app) { items.push(Box::new(sep)); }

    // Sessions submenu
    let sessions = state.lock_or_recover().session_summaries();
    let session_label = format!("{} ({})", i18n::t("sessions", &lang), sessions.len());
    let mut session_items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();
    if sessions.is_empty() {
        if let Ok(no) = MenuItem::with_id(&app, "ctx-none", i18n::t("noSessions", &lang), false, None::<&str>) {
            session_items.push(Box::new(no));
        }
    } else {
        for (sid, sess_state, _pid, agent) in &sessions {
            let icon = match sess_state.as_str() {
                "working" | "typing" => "⚡",
                "thinking" => "💭",
                "juggling" => "🎪",
                "idle" => "💤",
                "sleeping" => "😴",
                _ => "⚡",
            };
            let state_label = match sess_state.as_str() {
                "working" | "typing" => i18n::t("sessionWorking", &lang),
                "thinking" => i18n::t("sessionThinking", &lang),
                "juggling" => i18n::t("sessionJuggling", &lang),
                "idle" => i18n::t("sessionIdle", &lang),
                "sleeping" => i18n::t("sessionSleeping", &lang),
                _ => sess_state.clone(),
            };
            let label = format!("{icon} {agent}  {state_label}  {}", i18n::t("sessionJustNow", &lang));
            let item_id = format!("ctx-session-{}", sid);
            if let Ok(item) = MenuItem::with_id(&app, &item_id, &label, true, None::<&str>) {
                session_items.push(Box::new(item));
            }
        }
    }
    let sess_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = session_items.iter().map(|i| i.as_ref()).collect();
    if let Ok(sub) = Submenu::with_items(&app, &session_label, true, &sess_refs) {
        items.push(Box::new(sub));
    }

    if let Ok(sep) = PredefinedMenuItem::separator(&app) { items.push(Box::new(sep)); }

    // Language submenu (with checkmark)
    let en_label = if lang == "en" { "✓ English" } else { "English" };
    let zh_label = if lang == "zh" { "✓ 中文" } else { "中文" };
    if let (Ok(en), Ok(zh)) = (
        MenuItem::with_id(&app, "ctx-lang-en", en_label, true, None::<&str>),
        MenuItem::with_id(&app, "ctx-lang-zh", zh_label, true, None::<&str>),
    ) {
        if let Ok(sub) = Submenu::with_items(&app, i18n::t("language", &lang), true, &[&en, &zh]) {
            items.push(Box::new(sub));
        }
    }

    // About + Quit
    if let Ok(sep) = PredefinedMenuItem::separator(&app) { items.push(Box::new(sep)); }
    if let Ok(about) = MenuItem::with_id(&app, "ctx-about", i18n::t("about", &lang), true, None::<&str>) {
        items.push(Box::new(about));
    }
    if let Ok(q) = MenuItem::with_id(&app, "ctx-quit", i18n::t("quit", &lang), true, None::<&str>) {
        items.push(Box::new(q));
    }

    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = items.iter().map(|i| i.as_ref()).collect();
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

/// Shared DND toggle — used from tauri command, context menu, and tray handler.
pub(crate) fn do_toggle_dnd(app: &AppHandle, state: &SharedState) {
    let new_dnd = {
        let mut sm = state.lock_or_recover();
        sm.dnd = !sm.dnd;
        sm.dnd
    };
    let _ = app.emit("dnd-change", serde_json::json!({ "enabled": new_dnd }));
}

#[tauri::command]
fn toggle_dnd(app: AppHandle, state: tauri::State<'_, SharedState>) {
    do_toggle_dnd(&app, &state);
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

    let _ = app.emit("state-change", serde_json::json!({ "state": out_state, "svg": out_svg, "flip": flip }));
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
    if !is_mini { return false; }
    mini::should_snap_to_edge(app)
        .map(|s| s.side == mini::SnapSide::Left)
        .unwrap_or(false)
}

pub(crate) fn sync_hit(app: &AppHandle) {
    if let Some(bounds) = windows::get_pet_bounds(app) {
        windows::sync_hit_window(app, &bounds, &windows::HitBox::INTERACTIVE);
    }
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
fn trigger_sleep_sequence(app: AppHandle, state: tauri::State<'_, SharedState>, abort_handle: tauri::State<'_, SleepAbortHandle>) {
    cancel_pending_task(&abort_handle);

    transition(&app, &state, "yawning", "clyde-idle-yawn.svg");

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let handle = tauri::async_runtime::spawn(async move {
        // yawn → doze
        tokio::time::sleep(std::time::Duration::from_millis(YAWN_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "yawning" { return; }
        transition(&app2, &state2, "dozing", "clyde-idle-doze.svg");

        // doze → collapse
        tokio::time::sleep(std::time::Duration::from_millis(DOZE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "dozing" { return; }
        transition(&app2, &state2, "collapsing", "clyde-collapse-sleep.svg");

        // collapse → sleeping
        tokio::time::sleep(std::time::Duration::from_millis(COLLAPSE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "collapsing" { return; }
        transition(&app2, &state2, "sleeping", "clyde-sleeping.svg");
    });
    *abort_handle.lock_or_recover() = Some(handle);
}

#[tauri::command]
fn trigger_wake(app: AppHandle, state: tauri::State<'_, SharedState>, abort_handle: tauri::State<'_, SleepAbortHandle>) {
    cancel_pending_task(&abort_handle);

    let current = state.lock_or_recover().current_state.clone();
    if !matches!(current.as_str(), "yawning" | "dozing" | "collapsing" | "sleeping") {
        return;
    }

    transition(&app, &state, "waking", "clyde-wake.svg");

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(WAKE_DURATION_MS)).await;
        if state2.lock_or_recover().current_state != "waking" { return; }
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
    let session_id = id.strip_prefix("ctx-session-")
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
    // Strip optional "ctx-" prefix for backward compat with tray menu
    let action = id.strip_prefix("ctx-").unwrap_or(id);
    match action {
        "dnd"     => do_toggle_dnd(app, state),
        "mini"    => { if prefs::is_mini_mode(app) { mini::do_exit_mini(app); } else { mini::do_enter_mini(app); } }
        "size-s"  => tray::apply_size_pub(app, "S"),
        "size-m"  => tray::apply_size_pub(app, "M"),
        "size-l"  => tray::apply_size_pub(app, "L"),
        "lang-en" => tray::apply_lang_pub(app, "en"),
        "lang-zh" => tray::apply_lang_pub(app, "zh"),
        "about"   => { let _ = open::that("https://github.com/QingJ01/Clyde"); }
        "quit"    => app.exit(0),
        _ => {}
    }
}

fn setup_pet_window(app: &AppHandle, prefs: &prefs::Prefs) {
    let Some(pet) = app.get_webview_window("pet") else {
        eprintln!("Clyde: pet window not found!");
        return;
    };
    if let Err(e) = pet.set_background_color(Some(Color(0, 0, 0, 0))) {
        eprintln!("Clyde: set_background_color failed: {e}");
    }
    let _ = pet.set_ignore_cursor_events(true);
    let _ = pet.set_position(PhysicalPosition::new(prefs.x, prefs.y));
    let (w, h) = prefs::size_to_pixels(&prefs.size);
    let _ = pet.set_size(tauri::PhysicalSize::new(w, h));
    if let Err(e) = pet.show() {
        eprintln!("Clyde: pet.show() failed: {e}");
    }
    println!("Clyde: pet window shown ({}x{}) at ({},{})", w, h, prefs.x, prefs.y);
    #[cfg(debug_assertions)]
    pet.open_devtools();
}

fn setup_hit_window(app: &AppHandle, prefs: &prefs::Prefs) {
    if let Some(hit) = app.get_webview_window("hit") {
        // macOS needs a near-transparent background (not fully transparent)
        // to receive pointer events. Windows/Linux work with fully transparent.
        #[cfg(target_os = "macos")]
        let _ = hit.set_background_color(Some(Color(0, 0, 0, 1)));
        #[cfg(not(target_os = "macos"))]
        let _ = hit.set_background_color(Some(Color(0, 0, 0, 0)));
    }
    // Use prefs-based bounds instead of reading window geometry (avoids race
    // where the window hasn't fully rendered yet at startup).
    let bounds = windows::startup_pet_bounds(prefs);
    windows::sync_hit_window(app, &bounds, &windows::HitBox::INTERACTIVE);
    windows::show_hit_window(app);
    println!("Clyde: hit window synced to startup bounds");
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
        active: false, dragging: false, start_win_x: 0, start_win_y: 0,
        start_mouse_x: 0.0, start_mouse_y: 0.0,
    }));
    let shared_state: SharedState = Arc::new(Mutex::new(StateMachine::new()));
    let pending_perms: PendingPerms = Arc::new(Mutex::new(HashMap::new()));
    let shared_prefs: SharedPrefs = Arc::new(Mutex::new(prefs::Prefs::default()));
    let sleep_abort: SleepAbortHandle = Arc::new(Mutex::new(None));
    let bubble_map: permission::BubbleMap = Arc::new(Mutex::new(HashMap::new()));
    let mode_tracker: permission_mode::ModeTracker = Arc::new(Mutex::new(HashMap::new()));
    let shared_tray: tray::SharedTray = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .manage(drag_state)
        .manage(shared_state.clone())
        .manage(pending_perms.clone())
        .manage(shared_prefs.clone())
        .manage(sleep_abort.clone())
        .manage(bubble_map.clone())
        .manage(mode_tracker.clone())
        .manage(shared_tray.clone())
        .invoke_handler(tauri::generate_handler![
            drag_start, drag_move, drag_end, exit_mini_mode,
            hit_double_click, hit_flail, show_context_menu,
            toggle_dnd, mini_peek_in, mini_peek_out,
            http_server::resolve_permission,
            trigger_sleep_sequence,
            trigger_wake,
            set_window_size,
            set_lang,
            permission::get_bubble_data,
            permission::bubble_height_measured,
            permission::dismiss_bubble,
            focus::focus_terminal_for_session,
        ])
        .setup(move |app| {
            let prefs = prefs::load(app.handle());
            *shared_prefs.lock_or_recover() = prefs.clone();

            setup_pet_window(app.handle(), &prefs);
            setup_hit_window(app.handle(), &prefs);
            setup_tray(app.handle(), &prefs, &shared_tray);

            // Save position on close
            if let Some(pet_win) = app.get_webview_window("pet") {
                let handle_for_close = app.handle().clone();
                let shared_prefs_for_close = shared_prefs.clone();
                pet_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        if let Some(bounds) = windows::get_pet_bounds(&handle_for_close) {
                            let mut p = shared_prefs_for_close.lock_or_recover();
                            p.x = bounds.x;
                            p.y = bounds.y;
                            prefs::save(&handle_for_close, &p);
                        }
                    }
                });
            }

            // Start HTTP server + register hooks
            {
                let handle = app.handle().clone();
                let state_clone = shared_state.clone();
                let perms_clone = pending_perms.clone();
                let bubbles_clone = bubble_map.clone();
                let mode_clone = mode_tracker.clone();
                tauri::async_runtime::spawn(async move {
                    match http_server::start_server(handle.clone(), state_clone, perms_clone, bubbles_clone, mode_clone).await {
                        Some(port) => {
                            let installer = hooks::HookInstaller { settings_path: None, server_port: Some(port) };
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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
