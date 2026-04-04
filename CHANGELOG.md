# Changelog

## v0.1.6 — Update Check & Mini Mode Fix

### New Features

- **GitHub release update check** — Automatic check every 4 hours with bubble notification when a new version is available. "Download" opens the release page; "Skip" dismisses that version. Tray menu "Check for Updates" triggers an immediate manual check with feedback ("Already Up to Date" or error)
- **Update bubble UI** — Green-themed glassmorphism bubble with version badge and release notes preview

### Bug Fixes

- **Mini mode pet couldn't reach screen edge** — Peek detection fired during entry animation, cancelling it via AnimationGeneration. Fixed with PeekSuppressDeadline that waits until mouse exits vicinity
- **Mini mode pet bounced back out** — `reconcile_pet_geometry` fired on `WindowEvent::Moved` during animation, clamping pet to 120px visible (more than mini's 97px). Now skips reconcile when `mini_mode=true`
- **Mini mode peek oscillation** — Replaced simple cooldown with 3-phase state machine (Hidden → Peeking → Retracting) to prevent rapid peek/retract cycles
- **Tauri state collision** — Both `AnimationGeneration` and `PeekSuppressDeadline` were `Arc<AtomicU64>`, causing Tauri panic. Fixed with newtype wrapper
- **Fast-flick drag drops** — Added Pointer Capture (`setPointerCapture`) on hit window for reliable drag events

### Internal

- `update_check.rs` — new module: reqwest + rustls-tls for GitHub API, semver comparison, auto/manual check with bubble feedback
- `PeekSuppressDeadline` — newtype wrapper with `u64::MAX` sentinel for "wait until mouse exits"
- `PeekPhase` enum — Hidden/Peeking/Retracting state machine replacing simple boolean
- Near detection uses visible bounds (clamped to monitor) instead of full window bounds

---

## v0.1.5 — HiDPI Fix & Hit Region Precision

### New Features

- **Per-pose hit regions** (PR [#10](https://github.com/QingJ01/Clyde/pull/10) by [@0xC3B6](https://github.com/0xC3B6)) — Hit area now follows the pet's silhouette per animation pose (Standing, WorkingWide, Sleeping, Mini), replacing the single oversized rectangle. Reduces misclicks on transparent areas around the pet
- **Hit region transparency** — Hit window background is now fully transparent; macOS uses minimal alpha (`1/255`) only on hit zones to receive pointer events, eliminating the faint overlay visible on some displays

### Bug Fixes

- **Bubble windows positioned off-screen on HiDPI** — `measured_height` from frontend was in logical (CSS) pixels but positioning math used physical pixels. On 2× displays, bubbles were placed at half the intended distance from the pet, causing overlap or off-screen placement. All bubble positioning now consistently uses physical pixels with proper DPI conversion
- **Drag jump to top-left corner** (PR [#10](https://github.com/QingJ01/Clyde/pull/10)) — `drag_start` captured window position in logical coords while `drag_move` received physical from `toPhys()`. Now captures DPI scale at drag start and converts consistently to physical throughout the drag lifecycle
- **Snap tolerance shrank on HiDPI** — `SNAP_TOLERANCE` (30px) and `PEEK_OFFSET` (25px) were compared against physical-pixel coordinates without scaling. On 2× displays, snap zone was only 15 logical px. Now scaled by DPI factor
- **Drag clamp too permissive on HiDPI** — `MIN_VISIBLE` (30px) and `STARTUP_MIN_VISIBLE` (120px) were used as physical pixels. On 2× displays, only 15/60 logical pixels of the pet needed to remain visible. Now scaled by DPI factor
- **Position restore heuristic DPI-sensitive** — Thresholds (36px near-edge, 80px far-from-saved) in `should_restore_saved_single_monitor_position` were compared against physical coordinates. Now scaled by DPI
- **Eye tracking sluggish on HiDPI** — Cursor-to-pet distance was computed in physical pixels, making the normalization DPI-dependent. On 2× displays, `dist` was ~2× larger, suppressing eye movement. Now normalizes to logical space before directional calculation
- **Peek detection margin unscaled** — The 10px hover margin around the pet in mini mode was in physical pixels. On 2× displays, detection zone was only 5 logical px. Now scaled by DPI
- **Bubble initial size mismatch** — `inner_size` used 320px placeholder but `measured_height` used 200px, causing brief stacking glitch before real measurement arrived. Aligned to 200px

### Internal

- `windows::pet_scale_factor(app)` — new shared DPI utility, replacing duplicated `get_scale` in `permission.rs`
- `hit_regions.rs` — new module defining relative hit rectangles per animation profile
- `sync_hit` called on every state change to keep hit regions in sync with current pose
- Frontend `toPhys()` removed; Rust handles logical→physical conversion via captured `drag_scale_factor`

---

## v0.1.4 — Tray Hide, Permission Queue & Click-Through Fix

### New Features

- **Hide to Tray** — Clyde can be hidden from the desktop without quitting. Tray menu shows dynamic "Hide to Tray" / "Show Clyde" item; close button hides instead of exits when tray is present; left-click tray icon toggles visibility (Windows/Linux); incoming permission requests auto-restore the pet
- **macOS Space following** (PR [#8](https://github.com/QingJ01/Clyde/pull/8) by [@0xC3B6](https://github.com/0xC3B6)) — Pet, hit, and bubble windows follow the active desktop Space via `NSWindowCollectionBehaviorMoveToActiveSpace`
- **Permission request queue** — Multiple simultaneous requests are queued and shown one at a time; next request surfaces automatically when the current one is resolved
- **Elicitation hook support** — Claude Code's `Elicitation` hook (MCP structured input prompts) renders as a rich bubble form with enum choice lists, booleans, numbers, text, and textarea fields with validation
- **Configurable permission wait time** — New "Permission Wait Time" submenu (12 s / 20 s / 30 s / 45 s / 60 s) in tray and context menus, persisted to disk
- **Per-agent session badges** — Sessions in the right-click menu show colour-coded pills: orange for Claude, teal for Codex, blue for Copilot
- **Context menu expanded** — Opacity, position lock, click-through, auto-hide on fullscreen, and auto-DND during meetings are now accessible from the right-click context menu (previously tray-only)
- **Drag snap preview** — Dragging pet near a screen edge shows a blue highlight indicator before mini-mode triggers
- **Per-monitor snap-side memory** — Mini Mode remembers which edge (left/right) was last used on each monitor independently
- **Multi-monitor drag clamping** — Pet clamps to whichever monitor it overlaps most, not always the primary display
- **Auto-start gating** — "Start with Claude Code" toggle now writes `auto-start-config.json` sidecar so the hook respects the enabled/disabled state
- **Richer permission bubble metadata** — Bubbles display agent label, cleaned session summary, project folder name, and short session ID

### Bug Fixes

- **Click-through toggle double-fired** — Tauri v2 broadcasts menu events globally, so tray clicks reached both `handle_tray_event` and `handle_context_menu_event`, toggling twice and ending up unchanged. Context menu handler now requires `ctx-` prefix and ignores unprefixed tray items
- **Drag ghosting on high-DPI** — `drag_start` stored logical pixels while `drag_move` received physical from `toPhys()`, causing jump/drift on retina displays. Unified to physical pixels end-to-end
- **Bubble window shadow** — Permission bubbles had a native drop shadow; `set_shadow(false)` now called at creation
- **Tray left-click not working on Windows/Linux** — Tauri's default `show_menu_on_left_click(true)` intercepted left-click before the custom handler. Now set to `false`
- **Restore from tray broke click-through** — `do_show_from_tray` unconditionally showed hit window, ignoring click-through state. Now re-applies `apply_click_through` on restore
- **Auto-hide overrode manual hide-to-tray** — Dismissing a fullscreen window called `do_show_from_tray` even if user had manually hidden Clyde. Restore path now guarded with `is_hidden()`
- **Reminder bubble stuck after session advanced** — Watchdog now breaks early when session's `updated_at` advances; deferred `dismiss_transient_ui` cleans up stragglers
- **Permission summary showed `/resume` noise** — Summary cleaner now strips leading `/resume` token
- **Mini Mode snapped to primary monitor** — `should_snap_to_edge` used `primary_monitor()` instead of the current monitor. Now uses `monitor_for_bounds`
- **Mini Mode restored to wrong position after monitor change** — Position restore now prefers per-monitor placement entry, falling back to pre-mini coords only if none exists
- **Drag threshold used logical coords on HiDPI** — The 3px threshold was compared against logical coords while the pipeline used physical. Now consistently uses `toPhys()`
- **Drag position saved twice** — Duplicate save in `drag_end` could overwrite per-monitor placement data. Removed
- **BubbleCard Svelte warnings** — Fixed component warnings
- **Stuck reminder dismissal** — Fixed reminder not clearing properly

---

## v0.1.3 — Bug Fixes & Stability

### Bug Fixes

- **"Go to Terminal" button was silently broken** — frontend passed `sessionId` but Rust expected `pid`/`cwd`, so the button did nothing. Now accepts `session_id` and does PID lookup server-side.
- **Hit window not transparent on Windows** — `rgba(0,0,0,0.01)` background (needed for macOS pointer events) rendered as opaque gray on Windows WebView2. Now uses `#[cfg(target_os)]` to apply near-transparent only on macOS.
- **DPI coordinate mismatches (6 locations)** — `get_pet_monitor()` returned logical coords while everything else used physical, causing wrong snap detection, bubble positioning, and hit window clamp at DPI ≠ 100%. Unified to physical throughout; drag pipeline converts to logical only for clamp math.
- **Pet couldn't be dragged to top 1/4 of screen** — frontend's `toPhys()` doubled the DPI scaling on `screenX/screenY`. Removed `toPhys`; drag pipeline now works in logical coords with explicit physical conversion at `set_position`.
- **Cleanup loop held lock across emit_state** — latent deadlock risk. Now drops `SharedState` lock before calling `emit_state`.
- **Double lock in show_context_menu** — `SharedPrefs` locked twice in a row; `lang` could go stale. Combined into single lock.
- **ModeNotice dismiss left stale BubbleMap entry** — `window.close()` bypassed Rust cleanup. New `dismiss_bubble` Tauri command properly removes from BubbleMap.
- **Bubble centering used pet height instead of width** — correct only because pet is square. `get_pet_anchor` now returns `(x, y, width, height)`.
- **codex_monitor inconsistent lock** — raw `lock().unwrap_or_else` replaced with `lock_or_recover()`.

### Improvements

- **Hit window startup alignment** (from PR [#4](https://github.com/QingJ01/Clyde/pull/4) by [@0xC3B6](https://github.com/0xC3B6)) — use prefs-based bounds instead of re-reading window geometry (avoids macOS race condition)
- **Hit window resize alignment** — capture position before `set_size`, then sync with known position + new size
- **Position saved after every drag** — survives force-quit / task manager kill (previously only saved on `CloseRequested`)
- **Bubble constants scaled by DPI** — `BUBBLE_WIDTH`, `BUBBLE_MARGIN`, `BUBBLE_GAP` multiplied by `scale_factor` for correct positioning on HiDPI displays
- **Snap preview** — pet scales to 70% + 60% opacity when near screen edge during drag; 150ms ease-out transition
- **WSL Codex session monitoring** (PR [#5](https://github.com/QingJ01/Clyde/pull/5) by [@Lane0218](https://github.com/Lane0218)) — detect and monitor Codex sessions running inside WSL

### Closed Issues & PRs

- Closes [#3](https://github.com/QingJ01/Clyde/issues/3) — macOS 下启动或切换尺寸后 hit window 与 pet window 错位 (by [@0xC3B6](https://github.com/0xC3B6))
- Cherry-picked [#4](https://github.com/QingJ01/Clyde/pull/4) — fix: keep hit window aligned on startup and resize (by [@0xC3B6](https://github.com/0xC3B6))

---

## v0.1.2 — Multi-Monitor & macOS Fix

### Multi-Monitor Support

- Pet can now be dragged freely across all monitors (no longer clamped to primary screen)
- Edge snap detection uses current monitor bounds — works correctly on secondary monitors and monitors with negative coordinates (e.g. left-side displays)
- Mini mode hides behind the correct monitor edge, no more residual rendering on adjacent screens
- Hit window clamped to current monitor bounds

### Snap Preview

- Dragging pet into the edge snap zone (30px) now shows a visual preview: pet scales to 70% + 60% opacity
- Smooth 150ms ease-out transition in and out of the preview
- Preview clears immediately on drag release

### macOS Fixes (PR [#2](https://github.com/QingJ01/Clyde/pull/2) by [@kinoko-shelter](https://github.com/kinoko-shelter))

- Enable Tauri macOS private API for proper transparent window rendering
- Expand interactive hit area (`HitBox::INTERACTIVE`) so dragging works reliably across the full pet window
- Give hit window a near-transparent background (`rgba(0,0,0,0.01)`) to receive pointer events on macOS
- CI: ad-hoc code signing (`APPLE_SIGNING_IDENTITY='-'`) so Apple Silicon Macs can run without manual `codesign`

### Other

- Updated Troubleshooting docs: macOS "App is damaged" fix now includes `codesign --force --deep --sign -` for Apple Silicon
- Removed legacy Electron `build.yml` workflow

---

## v0.1.1 — Hook Format Fix

### Breaking Fix

- **All hooks now use nested `{matcher, hooks[]}` format** — Claude Code silently ignored the old flat `{type, command}` format. This was the root cause of hooks not firing for many users. Clyde now registers all 13 event hooks + PermissionRequest in the correct nested format, and auto-cleans old flat entries on startup.

### Improvements

- Context menu: sessions submenu with emoji status icons, size/language checkmarks, About button
- Bubble positioning: anchored to pet window instead of fixed screen corner, stacks above (or below if no room)
- Permission mode tracker: real-time awareness of Claude's permission mode with mode change notifications
- Codex monitor: scan nested date directories, correct event mapping, 1-hour file age filter, proper `agent_id = "Codex"` tagging
- Eye tracking: 80ms CSS ease-out transition for smooth cursor following
- Hit window: keyboard accessibility (Enter/Space), aria-labels, explicit pointer capture release
- Peek detection: symmetric 10px zone (was 30/10 asymmetric)
- Auto-focus: only steal focus on `attention` (task complete), not `notification`
- Mutex safety: `MutexExt::lock_or_recover()` replaces 50+ `.expect()` calls — prevents panic cascades
- `run()` split into `setup_pet_window`, `setup_hit_window`, `setup_tray`, `start_cleanup_loop`
- Animation duration constants, default size/screen constants centralized
- Hook installer: precise regex matching for flat hook cleanup, separate migration log, 9 JS tests
- macOS: conditional `transparent()`/`shadow()` with `#[cfg(not(target_os = "macos"))]`
- Web: official site with auto language detection, brand logos, mobile hamburger menu, friend links

### Bug Fixes

- Fix hooks not firing due to flat format (community report from LINUX DO)
- Fix DND mode not blocking permission bubbles
- Fix language menu missing current selection checkmark
- Fix right-click locking drag state (`e.button !== 0` filter)
- Remove legacy Electron `build.yml` workflow that conflicted with Tauri release
- Remove leftover upstream files (docs/, scripts/, extensions/)

---

## v0.1.0 — Initial Release

The first release of Clyde on Desk, a Tauri v2 rewrite of the original [Clawd on Desk](https://github.com/rullerzhou-afk/clawd-on-desk) project.

### Highlights

- **Tauri v2 + Rust + Svelte 5** — complete rewrite from Electron, ~5 MB bundle, <1s cold start, <30 MB memory
- **Multi-agent support** — Claude Code (hooks), Codex CLI (JSONL polling), Copilot CLI (hooks) — all three can run simultaneously
- **12 animated states** — idle eye-tracking, thinking, typing, building, juggling, conducting, error, happy, notification, sweeping, carrying, sleeping
- **Permission approval bubbles** — floating cards for Claude Code tool permissions with Allow / Deny / Suggestion rules
- **Permission mode tracking** — real-time awareness of Claude's permission mode (Default, Accept Edits, Bypass, Plan) with mode change notifications

### Features

**Animations & Interaction**
- 12 SVG animation states driven by real-time agent events
- Eye tracking with smooth CSS easing (80ms ease-out interpolation)
- Click reactions: double-click poke, 4-click flail
- Drag from any state with Pointer Capture (prevents fast-flick drops)
- Keyboard accessibility: Enter/Space on hit window, aria-labels on bubbles

**Mini Mode**
- Drag to screen edge or right-click to enter
- Pet hides behind edge, peeks on hover
- Shows mini alerts/celebrations while tucked away
- Symmetric 10px peek detection zone

**Permission System**
- HTTP hook receives Claude Code's PermissionRequest events
- Glassmorphism dark bubble UI with tool badge and input preview
- Suggestion buttons render readable labels (e.g. "Always allow Read")
- Structured `updatedPermissions` response format for suggestion rules
- Watchdog auto-dismisses bubbles after 5 minutes or when terminal answers first
- Bubbles anchor to pet position (not fixed screen corner)
- Permission mode tracker with 3-source priority (Hook > Transcript > Settings)
- Mode change notifications with 300ms debounce and 2s dedup

**Codex CLI Monitor**
- Polls `~/.codex/sessions/YYYY/MM/DD/*.jsonl` (nested date directories)
- Correct event mapping: `event_msg`, `response_item`, `function_call`, `task_complete`
- Only tracks sessions active within the last hour
- Sessions tagged with `agent_id = "Codex"` (no misidentification as Claude Code)

**Session Intelligence**
- Multi-session priority resolution (highest-priority state wins)
- Subagent awareness: 1 = juggling, 2+ = conducting
- Terminal focus via right-click session menu
- Auto-cleanup: 10-minute stale timeout, process liveness detection
- DND mode silences all events

**Context Menu**
- Sessions submenu with emoji status icons
- Size submenu with checkmark on current selection
- Language submenu with checkmark
- About button opens GitHub page

**System**
- System tray with full controls
- Position memory across restarts
- Single instance lock
- Auto-start with Claude Code (SessionStart hook)
- Chinese / English i18n

### Architecture

| Layer | Technology |
|-------|-----------|
| Desktop framework | Tauri v2 |
| Backend | Rust (state machine, HTTP server, window management) |
| Frontend | Svelte 5 (3 windows: pet, hit, bubble) |
| HTTP server | Axum on shared Tokio runtime |
| Build tool | Vite |

### Testing

- 35 Rust unit tests (state machine, hooks, permissions, positioning, i18n)
- 9 Node.js hook installer tests (migration, idempotency, isolation)
- Manual test scripts: `test-demo.sh`, `test-mini.sh`, `test-macos.sh`

### Known Limitations

| Limitation | Details |
|------------|---------|
| Codex: no terminal focus | JSONL polling doesn't carry terminal PID |
| Copilot: no permission bubble | Copilot hook protocol only supports deny |
| HTTP server unauthenticated | Binds `127.0.0.1` only; token auth planned |

### Credits

Forked from [Clawd on Desk](https://github.com/rullerzhou-afk/clawd-on-desk) by [@rullerzhou-afk](https://github.com/rullerzhou-afk). Pixel art reference from [clawd-tank](https://github.com/marciogranzotto/clawd-tank) by [@marciogranzotto](https://github.com/marciogranzotto).
