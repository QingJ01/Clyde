# Clyde on Desk — Code Review Fix Plan

Based on a 6-person review team (UX, maintainability, security, performance, bugs, devil's advocate).

---

## Stage 1: Critical Bugs & Security (P0)

**Goal**: Fix data-loss bugs, security holes, and silent failures.
**Tests**: `cargo test` pass + manual HTTP test
**Status**: Not Started

### 1.1 Fix port range mismatch (Rust 7 ports vs JS 5 ports)

**Problem**: `http_server.rs:172` tries ports 23333-23339 (7 ports), but `server-config.js:9` `SERVER_PORT_COUNT = 5` only scans 23333-23337. If Rust binds to 23338/23339, hooks can never discover the server.

**Fix**:
```
File: hooks/server-config.js:9
- const SERVER_PORT_COUNT = 5;
+ const SERVER_PORT_COUNT = 7;
```

### 1.2 Fix `close_bubble` double-close race condition

**Problem**: `permission.rs:57-63` — `close_bubble()` can be called twice (user click + scopeguard Drop). Second call operates on already-destroyed window.

**Fix** in `permission.rs`:
```rust
pub fn close_bubble(app: &AppHandle, bubbles: &BubbleMap, id: &str) {
    // Atomically remove from map first — if already removed, skip
    let removed = bubbles.lock().expect("bubble map poisoned").remove(id).is_some();
    if !removed { return; }
    if let Some(win) = app.get_webview_window(&format!("bubble-{id}")) {
        let _ = win.destroy();
    }
    reposition_bubbles(app, bubbles);
}
```

### 1.3 Fix hit window `w as u32` underflow

**Problem**: `windows.rs:73-74` — after clamping, `w` can be negative before the `w <= 0` check on line 76 if the pet is mostly off-screen. But between lines 74 and 76 there's no issue since the check IS there. However, the logic can be clearer and safer.

**Fix** in `windows.rs:73-79` — consolidate the clamp logic:
```rust
if let Some(monitor) = app.primary_monitor().ok().flatten() {
    let screen_w = monitor.size().width as i32;
    if x < 0 { w += x; x = 0; }
    if x + w > screen_w { w = screen_w - x; }
}
if w <= 0 { return; }
// Now safe to cast
let _ = hit_win.set_position(PhysicalPosition::new(x, y));
let _ = hit_win.set_size(PhysicalSize::new(w as u32, h));
```
The current code is actually safe (line 76 guards), but add a comment to make intent clear.

### 1.4 Fix Codex monitor file truncation handling

**Problem**: `codex_monitor.rs:42` — if a file is truncated/rotated, `file_len < offset`, and `file_len <= offset` skips all new content forever.

**Fix** in `codex_monitor.rs:33-42`:
```rust
let stored_offset = known_files.get(&path).copied().unwrap_or(0);
// Detect file truncation/rotation: restart from beginning
let offset = if file_len < stored_offset { 0 } else { stored_offset };
if file_len <= offset { continue; }
```

### 1.5 Fix `BubbleCard` suggestion parameter silently dropped

**Problem**: `BubbleCard.svelte:37` passes `suggestion` to `resolve_permission`, but Rust command (`http_server.rs:204`) doesn't accept it. Suggestion data is silently discarded.

**Fix** in `http_server.rs`:
```rust
#[tauri::command]
pub fn resolve_permission(
    app: tauri::AppHandle,
    pending: tauri::State<PendingPerms>,
    bubbles: tauri::State<permission::BubbleMap>,
    id: String,
    decision: String,
    suggestion: Option<String>,  // NEW
) {
    let tx = { pending.lock().expect("perms mutex poisoned").remove(&id) };
    if let Some(tx) = tx {
        let behavior = if let Some(ref sug) = suggestion {
            sug.clone()
        } else {
            decision
        };
        let _ = tx.send(PermDecision { behavior });
    }
    permission::close_bubble(&app, &bubbles, &id);
}
```

### 1.6 Fix hooks.rs JSON parse fallback losing user config

**Problem**: `hooks.rs:49` — `serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))` silently replaces malformed settings.json with `{}`, destroying all user config.

**Fix** in `hooks.rs:46-52`:
```rust
let mut settings: serde_json::Value = if settings_path.exists() {
    let raw = std::fs::read_to_string(settings_path)
        .context("reading settings.json")?;
    match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            // Backup before overwriting
            let backup = settings_path.with_extension("json.bak");
            let _ = std::fs::copy(settings_path, &backup);
            eprintln!("Clyde: settings.json parse error ({e}), backed up to {}", backup.display());
            serde_json::json!({})
        }
    }
} else {
    serde_json::json!({})
};
```

### 1.7 Fix hook dedup matching too broadly

**Problem**: `hooks.rs:81,88` — `cmd.contains("clawd")` and `cmd.contains("clyde")` would match any user command containing those substrings (e.g., `my-clyde-helper.js`).

**Fix** in `hooks.rs:77-96` — match on exact filenames, not substrings:
```rust
list.retain(|v| {
    if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
        // Remove our hooks by exact filename match
        let has_our_file = [HOOK_SCRIPT_NAME, AUTO_START_NAME, "clawd-hook.js"]
            .iter()
            .any(|name| {
                // Match "path/to/<name>" or "path\to\<name>"
                cmd.contains(&format!("/{name}")) || cmd.contains(&format!("\\{name}"))
                    || cmd.ends_with(name)
            });
        if has_our_file { return false; }
    }
    if let Some(hooks_arr) = v.get("hooks").and_then(|h| h.as_array()) {
        for hook in hooks_arr {
            if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                let has_our_file = [HOOK_SCRIPT_NAME, "clawd-hook.js"]
                    .iter()
                    .any(|name| cmd.contains(&format!("/{name}")) || cmd.contains(&format!("\\{name}")) || cmd.ends_with(name));
                if has_our_file { return false; }
            }
        }
    }
    true
});
```

---

## Stage 2: Performance & Blocking I/O (P1)

**Goal**: Fix async-blocking operations and reduce idle CPU.
**Tests**: `cargo test` pass + manual verify CPU in idle
**Status**: Not Started

### 2.1 Fix Codex monitor blocking I/O in async context

**Problem**: `codex_monitor.rs` uses `std::fs::read_dir`, `File::open`, `read_to_string` inside a `tokio::spawn` async task, blocking the tokio worker thread.

**Fix**: Wrap the entire scan loop body in `spawn_blocking`:
```rust
loop {
    interval.tick().await;
    let codex_dir = codex_dir.clone();
    let state = state.clone();
    let app = app.clone();
    let known = known_files.clone(); // use Arc<Mutex<HashMap>> or pass by value

    let new_known = tokio::task::spawn_blocking(move || {
        // ... existing scan logic, return updated known_files
    }).await.unwrap_or(known_files_snapshot);
    known_files = new_known;
}
```

Simpler alternative — since the entire function is a dedicated polling loop, just use `std::thread::spawn` instead of `tauri::async_runtime::spawn`:
```rust
pub fn start_codex_monitor(app: AppHandle, state: SharedState) {
    std::thread::Builder::new()
        .name("codex-monitor".into())
        .spawn(move || {
            // ... existing loop with std::thread::sleep instead of tokio interval
        })
        .expect("failed to spawn codex monitor thread");
}
```

### 2.2 Fix macOS/Linux `focus_window_by_pid` blocking async handler

**Problem**: `focus.rs:59,64-70` — macOS/Linux implementations use `Command::new(...).status()` which blocks. Called from async HTTP handler (`http_server.rs:113`).

**Fix** in `focus.rs` for macOS and Linux:
```rust
#[cfg(target_os = "macos")]
pub fn focus_window_by_pid(pid: u32, _cwd: &str) {
    let script = format!(
        r#"tell application "System Events"
            set theProcess to first process whose unix id is {}
            set frontmost of theProcess to true
        end tell"#,
        pid
    );
    // Non-blocking: spawn a thread like Windows does
    let _ = std::thread::Builder::new()
        .name("focus-window".into())
        .spawn(move || {
            let _ = Command::new("osascript").arg("-e").arg(&script).status();
        });
}

#[cfg(target_os = "linux")]
pub fn focus_window_by_pid(pid: u32, _cwd: &str) {
    let _ = std::thread::Builder::new()
        .name("focus-window".into())
        .spawn(move || {
            let result = Command::new("wmctrl")
                .args(["-ip", &pid.to_string()])
                .status();
            if result.map(|s| !s.success()).unwrap_or(true) {
                let _ = Command::new("xdotool")
                    .args(["search", "--pid", &pid.to_string(), "windowfocus"])
                    .status();
            }
        });
}
```

### 2.3 Merge tick.rs mini-mode mutex acquisitions

**Problem**: `tick.rs:105-116` — `tick_clone.lock()` acquired 3 separate times for peek detection in the same tick iteration.

**Fix** in `tick.rs:94-118` — merge into the existing lock block at line 68, or a single new block:
```rust
if is_mini {
    if let Some(bounds) = get_pet_bounds(&app) {
        let near = cx >= (bounds.x - 30) as f64
            && cx <= (bounds.x + bounds.width as i32 + 10) as f64
            && cy >= bounds.y as f64
            && cy <= (bounds.y + bounds.height as i32) as f64;
        let mut ts = tick_clone.lock().expect("tick state mutex poisoned");
        if near && !ts.is_peeking {
            ts.is_peeking = true;
            drop(ts);
            let _ = app.emit("mini-peek-in", ());
        } else if !near && ts.is_peeking {
            ts.is_peeking = false;
            drop(ts);
            let _ = app.emit("mini-peek-out", ());
        }
    }
} else {
    tick_clone.lock().expect("tick state mutex poisoned").is_peeking = false;
}
```

### 2.4 Remove redundant 200ms state sync task

**Problem**: `lib.rs:518-530` — a dedicated async task copies `current_state` from `SharedState` into `state_for_tick` every 200ms. Tick loop can read `SharedState` directly (lock hold time is just a String clone).

**Fix**: Remove the 200ms sync task and `state_for_tick`. Change `start_tick` signature:
```rust
// tick.rs
pub fn start_tick(app: AppHandle, state: SharedState) -> SharedTickState {
    // ...
    loop {
        interval.tick().await;
        // ...
        let state_str = state.lock().expect("state mutex poisoned").current_state.clone();
        // ... rest unchanged
    }
}
```

In `lib.rs:518-530`, delete the sync task and `state_for_tick`. Change line 560:
```rust
tick::start_tick(app.handle().clone(), shared_state.clone());
```

### 2.5 Reduce drag_move redundant bounds queries

**Problem**: `lib.rs:75-88` — `get_pet_bounds` called twice per `drag_move`. At 120-240Hz mouse events, this doubles IPC calls.

**Fix** in `lib.rs:68-88`:
```rust
let mut new_x = base_x + dx as i32;
let mut new_y = base_y + dy as i32;

if let Some(monitor) = app.primary_monitor().ok().flatten() {
    let screen_w = monitor.size().width as i32;
    let screen_h = monitor.size().height as i32;
    // Use drag state's initial size, don't query again
    let pet_w = (smx - base_x as f64).abs() as i32 + 200; // fallback
    if let Some(bounds) = windows::get_pet_bounds(&app) {
        let pet_w = bounds.width as i32;
        const MIN_VISIBLE: i32 = 30;
        new_x = new_x.max(MIN_VISIBLE - pet_w).min(screen_w - MIN_VISIBLE);
        new_y = new_y.max(0).min(screen_h - MIN_VISIBLE);

        // set_position then sync_hit using calculated new position
        if let Some(pet) = app.get_webview_window("pet") {
            let _ = pet.set_position(PhysicalPosition::new(new_x, new_y));
        }
        let updated_bounds = windows::WindowBounds {
            x: new_x, y: new_y, width: bounds.width, height: bounds.height,
        };
        windows::sync_hit_window(&app, &updated_bounds, &windows::HitBox::DEFAULT);
        return; // early return — skip the second get_pet_bounds below
    }
}

if let Some(pet) = app.get_webview_window("pet") {
    let _ = pet.set_position(PhysicalPosition::new(new_x, new_y));
}
if let Some(bounds) = windows::get_pet_bounds(&app) {
    windows::sync_hit_window(&app, &bounds, &windows::HitBox::DEFAULT);
}
```

---

## Stage 3: Code Deduplication & Maintainability (P1)

**Goal**: Eliminate duplicated code across modules.
**Tests**: `cargo test` pass
**Status**: Not Started

### 3.1 Extract shared `emit_state` and `sync_hit`

**Problem**: `emit_state` duplicated in `lib.rs:270` and `mini.rs:46`. `sync_hit` duplicated in `lib.rs:286` and `mini.rs:53`.

**Fix**: Keep the `lib.rs` versions as `pub(crate)` functions. Remove duplicates from `mini.rs`, import from `crate::`:
```rust
// lib.rs — make public within crate
pub(crate) fn emit_state(app: &AppHandle, state_str: &str, svg: &str) { ... }
pub(crate) fn sync_hit(app: &AppHandle) { ... }

// mini.rs — remove local definitions, use:
use crate::{emit_state, sync_hit};
```

### 3.2 Extract shared DND toggle logic

**Problem**: DND toggle repeated in `lib.rs:261-268` (command), `lib.rs:393-398` (context menu), `tray.rs` (tray handler).

**Fix**: Create one shared function in `lib.rs`:
```rust
pub(crate) fn toggle_dnd(app: &AppHandle, state: &SharedState) -> bool {
    let new_dnd = {
        let mut sm = state.lock().expect("state mutex poisoned");
        sm.dnd = !sm.dnd;
        sm.dnd
    };
    let _ = app.emit("dnd-change", serde_json::json!({ "enabled": new_dnd }));
    new_dnd
}
```

Then call `crate::toggle_dnd(&app, &state)` from all three places.

### 3.3 Extract shared state update+emit pipeline

**Problem**: `http_server.rs:72-108` and `codex_monitor.rs:59-66` both do `lock → update_session_state → resolve → svg_for_state → emit`, but codex_monitor skips DND check, oneshot handling, and flip.

**Fix**: Create a shared function in `state_machine.rs` or a new `events.rs`:
```rust
pub(crate) fn update_and_emit(
    app: &AppHandle,
    state: &SharedState,
    session_id: &str,
    state_str: &str,
    event: &str,
) {
    let (resolved, svg) = {
        let mut sm = state.lock().expect("state mutex poisoned");
        if sm.dnd && event != "SessionEnd" { return; }
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
```

### 3.4 Remove unused `_pet_h` variable

**File**: `lib.rs:77`
```rust
// Remove this line:
let _pet_h = bounds.as_ref().map(|b| b.height as i32).unwrap_or(200);
```

### 3.5 Remove suppressed `idle_ms` variable

**File**: `tick.rs:92`
```rust
// Remove this line:
let _ = idle_ms;
```
And change the destructuring at line 84 from `(idle_ms, should_yawn, should_wake)` to `(_idle_ms, should_yawn, should_wake)` or simply `(_, should_yawn, should_wake)`.

---

## Stage 4: UX Improvements (P2)

**Goal**: Improve visual feedback and interaction clarity.
**Tests**: Manual testing
**Status**: Not Started

### 4.1 Add DND visual feedback

**Problem**: Enabling DND doesn't change the pet's appearance. Users can't tell if DND is active.

**Fix** in `tray.rs` DND handler and `lib.rs` context menu handler — after toggling DND:
```rust
if new_dnd {
    emit_state(&app, "sleeping", "clyde-sleeping.svg");
} else {
    // Restore to current session state
    let sm = state.lock().expect("state mutex poisoned");
    let resolved = sm.resolve_display_state();
    let svg = sm.svg_for_state(&resolved);
    drop(sm);
    emit_state(&app, &resolved, &svg);
}
```

### 4.2 Fix bubble initial position overflow

**Problem**: `permission.rs:89` — with 5+ bubbles, `y` saturates to 0 and bubbles stack at screen top.

**Fix** in `permission.rs:85-91`:
```rust
fn initial_bubble_position(app: &AppHandle, bubbles: &BubbleMap) -> (u32, u32) {
    let (screen_w, screen_h) = get_work_area(app);
    let count = bubbles.lock().expect("bubble map poisoned").len() as u32;
    let x = screen_w.saturating_sub(BUBBLE_WIDTH + BUBBLE_MARGIN);
    let y = screen_h
        .saturating_sub(BUBBLE_MARGIN + 200 + count * (200 + BUBBLE_GAP))
        .max(50); // minimum 50px from top
    (x, y)
}
```

### 4.3 Add tray menu state indicators

**Problem**: Tray menu items (DND, Mini, Size, Language) don't show current state.

The context menu already has `"✓ "` prefix for DND and Mini (`lib.rs:193-208`). Apply same pattern to `tray.rs`:
```rust
// In tray.rs build_menu() — read current prefs/state and add checkmarks
let dnd_label = if is_dnd { format!("✓ {}", t("dnd", lang)) } else { t("dnd", lang) };
```

### 4.4 Add bubble card i18n

**Problem**: `BubbleCard.svelte` has hardcoded "Allow", "Deny", "Go to Terminal" etc.

**Fix**: Pass lang via Tauri command or URL query param. Add translations to `i18n.rs`:
```rust
("allow", "zh") => "允许".into(),
("deny", "zh") => "拒绝".into(),
("goToTerminal", "zh") => "前往终端".into(),
```

---

## Stage 5: Async Safety & Resource Cleanup (P2)

**Goal**: Fix async anti-patterns and resource leaks.
**Tests**: `cargo test` pass + long-running soak test
**Status**: Not Started

### 5.1 Cancel competing animation tasks

**Problem**: `mini.rs:168` — `animate_to_x` spawns fire-and-forget tasks. Rapid peek_in/peek_out can create multiple concurrent animations on the same window.

**Fix**: Add an animation abort handle to managed state:
```rust
// In lib.rs — add a new shared handle for animation tasks
pub type AnimAbortHandle = Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>;
```

Pass it to mini functions; cancel before starting a new animation.

### 5.2 Clean up runtime.json on exit

**Problem**: `http_server.rs` writes `~/.clyde/runtime.json` on startup but never deletes it on exit. Stale port info causes hooks to try wrong port first.

**Fix**: Add cleanup in the Tauri `on_exit` or `CloseRequested` handler in `lib.rs` setup:
```rust
// After the run() builder, or in a Drop guard:
if let Some(home) = dirs::home_dir() {
    let runtime = home.join(".clyde").join("runtime.json");
    let _ = std::fs::remove_file(runtime);
}
```

### 5.3 Clean Codex monitor `known_files` for deleted files

**Problem**: `codex_monitor.rs:17` — `known_files` HashMap never removes entries for deleted session files.

**Fix**: Add periodic cleanup at the end of each scan loop:
```rust
// After processing all entries:
known_files.retain(|path, _| path.exists());
```

### 5.4 Use `get_work_area` properly for bubble positioning

**Problem**: `permission.rs:93-98` — `get_work_area` returns full monitor size (`m.size()`), not accounting for taskbar. Bubbles may appear behind taskbar.

**Fix**: Use `m.position()` and `m.size()` to compute available area, or on Windows use `SystemParametersInfoW(SPI_GETWORKAREA, ...)`. As a simpler first step, subtract a fixed margin:
```rust
fn get_work_area(app: &AppHandle) -> (u32, u32) {
    app.primary_monitor()
        .ok().flatten()
        .map(|m| {
            // Approximate: subtract 48px for taskbar
            let h = m.size().height.saturating_sub(48);
            (m.size().width, h)
        })
        .unwrap_or((1920, 1032))
}
```

---

## Stage 6: Future Architecture (P3 — Discussion Items)

These are not immediate fixes but should be discussed for the next major version.

### 6.1 Introduce `PetState` enum to replace string states

Replace all `"idle"`, `"working"`, etc. strings with a Rust enum. This eliminates typo bugs and enables exhaustive match checking. Estimated: ~2 hours refactor.

### 6.2 Consider adaptive tick frequency

- Sleeping/DND state: reduce from 50ms to 500ms
- Mouse hasn't moved for 5s: reduce to 200ms
- Resume 50ms on mouse movement detection

### 6.3 Consider HTTP server authentication

Add a random token to `runtime.json` that hooks must include in requests. Simple but effective against local process attacks.

### 6.4 Consider merging pet + hit windows

Eliminate the constant `sync_hit_window` calls by handling cursor events and rendering in a single window. Requires Tauri changes for selective cursor-event ignoring by region.

### 6.5 Consider hook registration user confirmation

Show a one-time dialog on first install asking the user to confirm hook registration into `~/.claude/settings.json`.

---

## Quick Reference: Fix Priority Matrix

| # | Fix | Stage | Files | Risk |
|---|-----|-------|-------|------|
| 1.1 | Port range mismatch | 1 | server-config.js | Low |
| 1.2 | Bubble double-close | 1 | permission.rs | Low |
| 1.3 | Hit window underflow | 1 | windows.rs | Low |
| 1.4 | Codex file truncation | 1 | codex_monitor.rs | Low |
| 1.5 | Suggestion param dropped | 1 | http_server.rs | Low |
| 1.6 | Settings.json data loss | 1 | hooks.rs | Medium |
| 1.7 | Hook dedup too broad | 1 | hooks.rs | Medium |
| 2.1 | Codex blocking I/O | 2 | codex_monitor.rs | Medium |
| 2.2 | Focus blocking async | 2 | focus.rs | Low |
| 2.3 | Tick mutex contention | 2 | tick.rs | Low |
| 2.4 | Remove 200ms sync task | 2 | lib.rs, tick.rs | Medium |
| 2.5 | Drag bounds queries | 2 | lib.rs | Low |
| 3.1 | Dedup emit_state/sync_hit | 3 | lib.rs, mini.rs | Low |
| 3.2 | Dedup DND toggle | 3 | lib.rs, tray.rs | Low |
| 3.3 | Dedup state update pipeline | 3 | http_server, codex_monitor | Medium |
| 3.4 | Remove unused _pet_h | 3 | lib.rs | None |
| 3.5 | Remove suppressed idle_ms | 3 | tick.rs | None |
| 4.1 | DND visual feedback | 4 | tray.rs, lib.rs | Low |
| 4.2 | Bubble position overflow | 4 | permission.rs | Low |
| 4.3 | Tray state indicators | 4 | tray.rs | Low |
| 4.4 | Bubble i18n | 4 | BubbleCard.svelte, i18n.rs | Low |
| 5.1 | Animation abort handle | 5 | mini.rs, lib.rs | Medium |
| 5.2 | Runtime.json cleanup | 5 | lib.rs | Low |
| 5.3 | known_files cleanup | 5 | codex_monitor.rs | Low |
| 5.4 | Work area vs full screen | 5 | permission.rs | Low |
