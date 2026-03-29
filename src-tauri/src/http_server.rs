use std::future::IntoFuture;

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tauri::AppHandle;
use crate::state_machine::{SharedState, ONESHOT_STATES};
use crate::util::MutexExt;
use crate::permission;

pub const CLYDE_SERVER_HEADER: &str = "x-clyde-server";
pub const CLYDE_SERVER_ID:     &str = "clyde-on-desk";
pub const DEFAULT_PORT:        u16  = 23333;

pub type PermSender   = oneshot::Sender<PermDecision>;
pub type PendingPerms = Arc<Mutex<HashMap<String, PermSender>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermDecision {
    pub behavior: String,
}

#[derive(Clone)]
struct ServerCtx {
    state:         SharedState,
    pending_perms: PendingPerms,
    app:           AppHandle,
    bubble_map:    permission::BubbleMap,
}

#[derive(Deserialize)]
struct StatePayload {
    state:      String,
    #[allow(dead_code)]
    svg:        Option<String>,
    session_id: Option<String>,
    event:      Option<String>,
    source_pid: Option<u32>,
    cwd:        Option<String>,
    agent_id:   Option<String>,
}

// NOTE: We accept raw JSON for permission requests because Claude Code's
// PermissionRequest hook payload format may vary. Fields are extracted manually.

async fn health(AxumState(_ctx): AxumState<ServerCtx>) -> Json<Value> {
    Json(json!({ "ok": true, "app": CLYDE_SERVER_ID }))
}

async fn post_state(
    AxumState(ctx): AxumState<ServerCtx>,
    Json(payload): Json<StatePayload>,
) -> (StatusCode, HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(CLYDE_SERVER_HEADER, CLYDE_SERVER_ID.parse().unwrap());

    let sid   = payload.session_id.unwrap_or_else(|| "default".into());
    let event = payload.event.unwrap_or_default();

    let (new_state, new_svg) = {
        let mut sm = ctx.state.lock_or_recover();
        // DND mode: skip state updates except SessionEnd
        if sm.dnd && event != "SessionEnd" {
            let mut headers = HeaderMap::new();
            headers.insert(CLYDE_SERVER_HEADER, CLYDE_SERVER_ID.parse().unwrap());
            return (StatusCode::OK, headers, "ok (dnd)".into());
        }
        if event == "SessionEnd" {
            sm.handle_session_end(&sid);
        } else {
            sm.update_session_state(&sid, &payload.state, &event);
            // Store metadata on the session entry
            if let Some(entry) = sm.sessions.get_mut(&sid) {
                if let Some(pid) = payload.source_pid { entry.source_pid = Some(pid); }
                if let Some(ref cwd) = payload.cwd { entry.cwd = cwd.clone(); }
                if let Some(ref aid) = payload.agent_id { entry.agent_id = aid.clone(); }
            }
        }
        let resolved = sm.resolve_display_state();
        // For SessionEnd, the payload state is semantically meaningless — skip oneshot branch (IMPORTANT-2)
        let is_session_end = event == "SessionEnd";
        let svg      = if !is_session_end && ONESHOT_STATES.contains(&payload.state.as_str()) {
            sm.svg_for_state(&payload.state)
        } else {
            sm.svg_for_state(&resolved)
        };
        sm.current_state = if !is_session_end && ONESHOT_STATES.contains(&payload.state.as_str()) {
            payload.state.clone()
        } else {
            resolved.clone()
        };
        sm.current_svg = svg.clone();
        (sm.current_state.clone(), svg)
    };

    crate::emit_state(&ctx.app, &new_state, &new_svg);

    // Auto-focus terminal only on "attention" (task complete).
    // "notification" is informational — don't steal focus for it.
    if payload.state == "attention" {
        if let Some(pid) = payload.source_pid {
            crate::focus::focus_window_by_pid(pid, payload.cwd.as_deref().unwrap_or(""));
        }
    }

    (StatusCode::OK, headers, "ok".into())
}

async fn post_permission(
    AxumState(ctx): AxumState<ServerCtx>,
    Json(payload): Json<Value>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(CLYDE_SERVER_HEADER, CLYDE_SERVER_ID.parse().unwrap());

    // Log raw payload for debugging field name mismatches
    eprintln!("Clyde: /permission payload keys: {:?}",
        payload.as_object().map(|o| o.keys().collect::<Vec<_>>()).unwrap_or_default());

    // Extract fields — try both snake_case and camelCase variants
    let tool_name = payload.get("tool_name").or_else(|| payload.get("toolName"))
        .and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let tool_input = payload.get("tool_input").or_else(|| payload.get("toolInput"))
        .cloned().unwrap_or(json!({}));
    let session_id = payload.get("session_id").or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let suggestions = payload.get("permission_suggestions")
        .or_else(|| payload.get("permissionSuggestions"))
        .and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let entry_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<PermDecision>();

    let bubble_data = permission::BubbleData {
        id: entry_id.clone(),
        tool_name,
        tool_input,
        suggestions,
        session_id,
        is_elicitation: false,
    };

    let bubble_opened = permission::show_bubble(&ctx.app, &ctx.bubble_map, bubble_data);
    if !bubble_opened {
        return (StatusCode::OK, headers, Json(perm_response("deny")));
    }
    ctx.pending_perms.lock_or_recover().insert(entry_id.clone(), tx);

    // Wait for user to click in bubble, or timeout.
    // Auto-close: a background watcher (see spawn below) checks if the HTTP client
    // disconnected by probing whether the tx was consumed. When the terminal answers
    // first, Claude Code drops the TCP connection. Since Axum does NOT cancel handlers
    // on disconnect, we spawn a watchdog that closes the bubble and drops tx after
    // detecting no resolution within a short grace period after session state changes.
    let watchdog_ctx = ctx.clone();
    let watchdog_id = entry_id.clone();
    let watchdog = tauri::async_runtime::spawn(async move {
        // Poll: if the pending_perm entry was removed (by resolve_permission from bubble click),
        // this task is orphaned and will just exit. Otherwise, close bubble on timeout.
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            interval.tick().await;
            if tokio::time::Instant::now() > deadline { break; }
            // Check if the entry was already resolved
            let still_pending = watchdog_ctx.pending_perms
                .lock_or_recover()
                .contains_key(&watchdog_id);
            if !still_pending { return; } // resolved by user click — nothing to do
        }
        // Timeout: close bubble and send default deny
        if let Some(tx) = watchdog_ctx.pending_perms.lock_or_recover().remove(&watchdog_id) {
            let _ = tx.send(PermDecision { behavior: "deny".into() });
        }
        permission::close_bubble(&watchdog_ctx.app, &watchdog_ctx.bubble_map, &watchdog_id);
    });

    let decision = rx.await.unwrap_or(PermDecision { behavior: "deny".into() });

    // Clean up
    watchdog.abort();
    ctx.pending_perms.lock_or_recover().remove(&entry_id);
    permission::close_bubble(&ctx.app, &ctx.bubble_map, &entry_id);

    (StatusCode::OK, headers, Json(perm_response(&decision.behavior)))
}

/// Build the response format Claude Code expects for PermissionRequest HTTP hooks.
fn perm_response(behavior: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {
                "behavior": behavior
            }
        }
    })
}

pub async fn start_server(app: AppHandle, state: SharedState, pending_perms: PendingPerms, bubble_map: permission::BubbleMap) -> Option<u16> {
    let ctx = ServerCtx { state, pending_perms, app, bubble_map };

    let router = Router::new()
        .route("/state",      get(health))
        .route("/state",      post(post_state))
        .route("/permission", post(post_permission))
        .with_state(ctx);

    for port in DEFAULT_PORT..DEFAULT_PORT + 7 {
        let addr = format!("127.0.0.1:{port}");
        if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
            let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
            tauri::async_runtime::spawn(axum::serve(listener, router).into_future());
            write_runtime_port(actual_port);
            println!("Clyde: HTTP server listening on 127.0.0.1:{actual_port}");
            return Some(actual_port);
        }
    }
    eprintln!("Clyde: no available ports in range {DEFAULT_PORT}-{}", DEFAULT_PORT + 6);
    None
}

fn write_runtime_port(port: u16) {
    if let Some(home) = dirs::home_dir() {
        // Write ~/.clyde/runtime.json in the format server-config.js expects
        let dir = home.join(".clyde");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("runtime.json");
        let json = serde_json::json!({
            "app": CLYDE_SERVER_ID,
            "port": port
        });
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&json).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[tauri::command]
pub fn resolve_permission(
    app: tauri::AppHandle,
    pending: tauri::State<PendingPerms>,
    bubbles: tauri::State<permission::BubbleMap>,
    id: String,
    decision: String,
    suggestion: Option<String>,
) {
    let tx = { pending.lock_or_recover().remove(&id) };
    if let Some(tx) = tx {
        // If a suggestion was selected, use it as the behavior; otherwise use the decision.
        let behavior = suggestion.unwrap_or(decision);
        let _ = tx.send(PermDecision { behavior });
    }
    // Close the bubble window — Rust owns window lifecycle (CRITICAL-1)
    permission::close_bubble(&app, &bubbles, &id);
}
