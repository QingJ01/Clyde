# Changelog

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
