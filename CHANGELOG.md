# Changelog

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
