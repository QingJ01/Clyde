use std::future::IntoFuture;

use crate::permission;
use crate::session_meta;
use crate::state_machine::{SharedState, ONESHOT_STATES};
use crate::util::MutexExt;
use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio::sync::oneshot;

pub const CLYDE_SERVER_HEADER: &str = "x-clyde-server";
pub const CLYDE_SERVER_ID: &str = "clyde-on-desk";
pub const DEFAULT_PORT: u16 = 23333;

pub type PermSender = oneshot::Sender<PermDecision>;
pub type PendingPerms = Arc<Mutex<HashMap<String, PermSender>>>;
pub type ApprovalQueue = Arc<Mutex<ApprovalQueueState>>;

#[derive(Debug, Clone)]
pub enum PermDecision {
    Allow,
    Deny,
    AllowWithPermissions(Vec<serde_json::Value>),
}

#[derive(Default)]
pub struct ApprovalQueueState {
    active_request_id: Option<String>,
    queued_request_ids: VecDeque<String>,
    request_data: HashMap<String, permission::BubbleData>,
}

#[derive(Clone)]
struct ServerCtx {
    state: SharedState,
    pending_perms: PendingPerms,
    approval_queue: ApprovalQueue,
    app: AppHandle,
    bubble_map: permission::BubbleMap,
    mode_tracker: crate::permission_mode::ModeTracker,
}

#[derive(Deserialize)]
struct StatePayload {
    state: String,
    #[allow(dead_code)]
    svg: Option<String>,
    session_id: Option<String>,
    event: Option<String>,
    source_pid: Option<u32>,
    cwd: Option<String>,
    agent_id: Option<String>,
    permission_mode: Option<String>,
}

#[derive(Deserialize)]
struct ClearPermissionPayload {
    #[serde(default)]
    session_ids: Vec<String>,
    #[serde(default)]
    demo_only: bool,
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

    let sid = payload.session_id.unwrap_or_else(|| "default".into());
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
                if let Some(pid) = payload.source_pid {
                    entry.source_pid = Some(pid);
                }
                if let Some(ref cwd) = payload.cwd {
                    entry.cwd = cwd.clone();
                }
                if let Some(ref aid) = payload.agent_id {
                    entry.agent_id = aid.clone();
                }
            }
        }
        let resolved = sm.resolve_display_state();
        // For SessionEnd, the payload state is semantically meaningless — skip oneshot branch (IMPORTANT-2)
        let is_session_end = event == "SessionEnd";
        let svg = if !is_session_end && ONESHOT_STATES.contains(&payload.state.as_str()) {
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

    // Update permission mode if provided
    if let Some(ref mode) = payload.permission_mode {
        use tauri::Manager;
        let lang = ctx
            .app
            .try_state::<crate::prefs::SharedPrefs>()
            .map(|p: tauri::State<crate::prefs::SharedPrefs>| p.lock_or_recover().lang.clone())
            .unwrap_or_else(|| "en".into());
        crate::permission_mode::update_session_mode(
            &ctx.app,
            &ctx.mode_tracker,
            &sid,
            mode,
            crate::permission_mode::ModeSource::Hook,
            &lang,
        );
    }

    // Auto-focus terminal only on "attention" (task complete).
    // "notification" is informational — don't steal focus for it.
    if payload.state == "attention" {
        if let Some(pid) = payload.source_pid {
            crate::focus::focus_window_by_pid(pid, payload.cwd.as_deref().unwrap_or(""));
        }
        let app = ctx.app.clone();
        let state = ctx.state.clone();
        let bubbles = ctx.bubble_map.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            crate::dismiss_transient_ui(&app, &state, &bubbles);
        });
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
    eprintln!(
        "Clyde: /permission payload keys: {:?}",
        payload
            .as_object()
            .map(|o| o.keys().collect::<Vec<_>>())
            .unwrap_or_default()
    );

    // Extract fields — try both snake_case and camelCase variants
    let tool_name = payload
        .get("tool_name")
        .or_else(|| payload.get("toolName"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let tool_input = payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .cloned()
        .unwrap_or(json!({}));
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let suggestions = payload
        .get("permission_suggestions")
        .or_else(|| payload.get("permissionSuggestions"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let agent_label_override = payload
        .get("agent_label")
        .or_else(|| payload.get("agentLabel"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let session_summary_override = payload
        .get("session_summary")
        .or_else(|| payload.get("sessionSummary"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let session_project_override = payload
        .get("session_project")
        .or_else(|| payload.get("sessionProject"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let session_short_id_override = payload
        .get("session_short_id")
        .or_else(|| payload.get("sessionShortId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let fallback_agent = payload
        .get("agent_id")
        .or_else(|| payload.get("agentId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "claude-code".to_string());
    let fallback_cwd = session_meta::extract_tool_cwd(&tool_input);
    let display = session_meta::ensure_session_display_meta(
        &ctx.state,
        &session_id,
        Some(fallback_agent.as_str()),
        fallback_cwd.as_deref(),
    );
    let agent_label = agent_label_override.unwrap_or(display.agent_label);
    let raw_session_summary = session_summary_override.unwrap_or(display.summary);
    let session_summary = session_meta::clean_resume_summary(&raw_session_summary);
    let session_project = session_project_override.unwrap_or(display.project);
    let session_short_id = session_short_id_override.unwrap_or(display.short_id);

    let entry_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<PermDecision>();
    ctx.pending_perms
        .lock_or_recover()
        .insert(entry_id.clone(), tx);

    let bubble_data = permission::BubbleData {
        id: entry_id.clone(),
        window_kind: permission::WindowKind::ApprovalRequest,
        tool_name,
        tool_input,
        suggestions,
        session_id,
        agent_label,
        session_summary,
        session_project,
        session_short_id,
        is_elicitation: false,
        mode_label: None,
        mode_description: None,
    };
    let bubble_session_id = bubble_data.session_id.clone();

    if !enqueue_permission_request(&ctx, bubble_data) {
        return (
            StatusCode::OK,
            headers,
            Json(perm_response(&PermDecision::Deny)),
        );
    }

    // Wait for user to click in bubble, or timeout.
    // Auto-close: a background watcher (see spawn below) checks if the HTTP client
    // disconnected by probing whether the tx was consumed. When the terminal answers
    // first, Claude Code drops the TCP connection. Since Axum does NOT cancel handlers
    // on disconnect, we spawn a watchdog that closes the bubble and drops tx after
    // detecting no resolution within a short grace period after session state changes.
    let watchdog_ctx = ctx.clone();
    let watchdog_id = entry_id.clone();
    let watchdog_session_id = bubble_session_id;
    let opened_at = std::time::Instant::now();
    let session_existed_at_open = {
        let sm = ctx.state.lock_or_recover();
        sm.sessions.contains_key(&watchdog_session_id)
    };
    let watchdog = tauri::async_runtime::spawn(async move {
        // Poll: if the pending_perm entry was removed (by resolve_permission from bubble click),
        // this task is orphaned and will just exit. Otherwise, close bubble on timeout or
        // when the session advances, which usually means the terminal handled the prompt.
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            interval.tick().await;
            // Check if the entry was already resolved
            let still_pending = watchdog_ctx
                .pending_perms
                .lock_or_recover()
                .contains_key(&watchdog_id);
            if !still_pending {
                return;
            } // resolved by user click — nothing to do
            let session_advanced = {
                let sm = watchdog_ctx.state.lock_or_recover();
                match sm.sessions.get(&watchdog_session_id) {
                    Some(entry) => entry.updated_at > opened_at,
                    None => session_existed_at_open,
                }
            };
            if session_advanced || tokio::time::Instant::now() > deadline {
                break;
            }
        }
        // Timeout: close bubble and send default deny
        if let Some(tx) = watchdog_ctx
            .pending_perms
            .lock_or_recover()
            .remove(&watchdog_id)
        {
            let _ = tx.send(PermDecision::Deny);
        }
        close_permission_request_ui(
            &watchdog_ctx.app,
            &watchdog_ctx.pending_perms,
            &watchdog_ctx.approval_queue,
            &watchdog_ctx.bubble_map,
            &watchdog_id,
        );
    });

    let decision = rx.await.unwrap_or(PermDecision::Deny);

    // Clean up
    watchdog.abort();
    ctx.pending_perms.lock_or_recover().remove(&entry_id);
    close_permission_request_ui(
        &ctx.app,
        &ctx.pending_perms,
        &ctx.approval_queue,
        &ctx.bubble_map,
        &entry_id,
    );

    (StatusCode::OK, headers, Json(perm_response(&decision)))
}

async fn clear_permission_debug(
    AxumState(ctx): AxumState<ServerCtx>,
    Json(payload): Json<ClearPermissionPayload>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(CLYDE_SERVER_HEADER, CLYDE_SERVER_ID.parse().unwrap());

    let ids: Vec<String> = {
        let queue = ctx.approval_queue.lock_or_recover();
        queue
            .request_data
            .iter()
            .filter(|(_, data)| {
                (!payload.session_ids.is_empty()
                    && payload
                        .session_ids
                        .iter()
                        .any(|sid| sid == &data.session_id))
                    || (payload.demo_only && data.session_id.contains("-demo-"))
            })
            .map(|(id, _)| id.clone())
            .collect()
    };

    for id in &ids {
        cancel_permission_request(
            &ctx.app,
            &ctx.pending_perms,
            &ctx.approval_queue,
            &ctx.bubble_map,
            id,
        );
    }

    (
        StatusCode::OK,
        headers,
        Json(json!({
            "ok": true,
            "cleared": ids.len(),
        })),
    )
}

fn enqueue_permission_request(ctx: &ServerCtx, bubble_data: permission::BubbleData) -> bool {
    let entry_id = bubble_data.id.clone();
    let should_show_now = {
        let mut queue = ctx.approval_queue.lock_or_recover();
        queue
            .request_data
            .insert(entry_id.clone(), bubble_data.clone());
        if queue.active_request_id.is_none() {
            queue.active_request_id = Some(entry_id);
            true
        } else {
            queue.queued_request_ids.push_back(bubble_data.id.clone());
            false
        }
    };

    if should_show_now {
        show_permission_or_deny(
            &ctx.app,
            &ctx.pending_perms,
            &ctx.approval_queue,
            &ctx.bubble_map,
            bubble_data,
        )
    } else {
        true
    }
}

fn show_permission_or_deny(
    app: &AppHandle,
    pending_perms: &PendingPerms,
    approval_queue: &ApprovalQueue,
    bubble_map: &permission::BubbleMap,
    bubble_data: permission::BubbleData,
) -> bool {
    if permission::show_bubble(app, bubble_map, bubble_data.clone()) {
        return true;
    }

    if let Some(tx) = pending_perms.lock_or_recover().remove(&bubble_data.id) {
        let _ = tx.send(PermDecision::Deny);
    }

    {
        let mut queue = approval_queue.lock_or_recover();
        queue.request_data.remove(&bubble_data.id);
        if queue.active_request_id.as_deref() == Some(bubble_data.id.as_str()) {
            queue.active_request_id = None;
        } else {
            queue
                .queued_request_ids
                .retain(|queued_id| queued_id != &bubble_data.id);
        }
    }

    activate_next_permission(app, pending_perms, approval_queue, bubble_map);
    false
}

fn close_permission_request_ui(
    app: &AppHandle,
    pending_perms: &PendingPerms,
    approval_queue: &ApprovalQueue,
    bubble_map: &permission::BubbleMap,
    id: &str,
) {
    let was_active = {
        let mut queue = approval_queue.lock_or_recover();
        queue.request_data.remove(id);
        if queue.active_request_id.as_deref() == Some(id) {
            queue.active_request_id = None;
            true
        } else {
            queue.queued_request_ids.retain(|queued_id| queued_id != id);
            false
        }
    };

    permission::close_bubble(app, bubble_map, id);

    if was_active {
        activate_next_permission(app, pending_perms, approval_queue, bubble_map);
    }
}

fn cancel_permission_request(
    app: &AppHandle,
    pending_perms: &PendingPerms,
    approval_queue: &ApprovalQueue,
    bubble_map: &permission::BubbleMap,
    id: &str,
) {
    if let Some(tx) = pending_perms.lock_or_recover().remove(id) {
        let _ = tx.send(PermDecision::Deny);
    }
    close_permission_request_ui(app, pending_perms, approval_queue, bubble_map, id);
}

fn activate_next_permission(
    app: &AppHandle,
    pending_perms: &PendingPerms,
    approval_queue: &ApprovalQueue,
    bubble_map: &permission::BubbleMap,
) {
    loop {
        let next_bubble = {
            let mut queue = approval_queue.lock_or_recover();
            let next_id = match queue.queued_request_ids.pop_front() {
                Some(id) => id,
                None => {
                    queue.active_request_id = None;
                    return;
                }
            };
            match queue.request_data.get(&next_id).cloned() {
                Some(data) => {
                    queue.active_request_id = Some(next_id);
                    data
                }
                None => continue,
            }
        };

        if show_permission_or_deny(app, pending_perms, approval_queue, bubble_map, next_bubble) {
            return;
        }
    }
}

/// Build the response format Claude Code expects for PermissionRequest HTTP hooks.
fn perm_response(decision: &PermDecision) -> Value {
    let decision_obj = match decision {
        PermDecision::Allow => json!({ "behavior": "allow" }),
        PermDecision::Deny => json!({ "behavior": "deny" }),
        PermDecision::AllowWithPermissions(perms) => json!({
            "behavior": "allow",
            "updatedPermissions": perms
        }),
    };
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision_obj
        }
    })
}

pub async fn start_server(
    app: AppHandle,
    state: SharedState,
    pending_perms: PendingPerms,
    approval_queue: ApprovalQueue,
    bubble_map: permission::BubbleMap,
    mode_tracker: crate::permission_mode::ModeTracker,
) -> Option<u16> {
    let ctx = ServerCtx {
        state,
        pending_perms,
        approval_queue,
        app,
        bubble_map,
        mode_tracker,
    };

    let router = Router::new()
        .route("/state", get(health))
        .route("/state", post(post_state))
        .route("/permission", post(post_permission))
        .route("/permission/debug/clear", post(clear_permission_debug))
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
    eprintln!(
        "Clyde: no available ports in range {DEFAULT_PORT}-{}",
        DEFAULT_PORT + 6
    );
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
        if std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&json).unwrap_or_default(),
        )
        .is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[tauri::command]
pub fn resolve_permission(
    app: tauri::AppHandle,
    pending: tauri::State<PendingPerms>,
    approval_queue: tauri::State<ApprovalQueue>,
    bubbles: tauri::State<permission::BubbleMap>,
    id: String,
    decision: String,
    selected_suggestion: Option<serde_json::Value>,
) {
    let tx = { pending.lock_or_recover().remove(&id) };
    if let Some(tx) = tx {
        let perm_decision = match (decision.as_str(), selected_suggestion) {
            ("allow", Some(sug)) => PermDecision::AllowWithPermissions(vec![sug]),
            ("allow", None) => PermDecision::Allow,
            _ => PermDecision::Deny,
        };
        let _ = tx.send(perm_decision);
    }
    close_permission_request_ui(&app, &pending, &approval_queue, &bubbles, &id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perm_response_allow() {
        let resp = perm_response(&PermDecision::Allow);
        let behavior = resp["hookSpecificOutput"]["decision"]["behavior"]
            .as_str()
            .unwrap();
        assert_eq!(behavior, "allow");
        assert!(resp["hookSpecificOutput"]["decision"]
            .get("updatedPermissions")
            .is_none());
    }

    #[test]
    fn test_perm_response_deny() {
        let resp = perm_response(&PermDecision::Deny);
        let behavior = resp["hookSpecificOutput"]["decision"]["behavior"]
            .as_str()
            .unwrap();
        assert_eq!(behavior, "deny");
    }

    #[test]
    fn test_perm_response_with_permissions() {
        let suggestion = json!({
            "type": "addRules",
            "rules": [{ "tool_name": "Read", "behavior": "allow" }]
        });
        let resp = perm_response(&PermDecision::AllowWithPermissions(
            vec![suggestion.clone()],
        ));
        let decision = &resp["hookSpecificOutput"]["decision"];
        assert_eq!(decision["behavior"].as_str().unwrap(), "allow");
        let perms = decision["updatedPermissions"].as_array().unwrap();
        assert_eq!(perms.len(), 1);
        assert_eq!(perms[0]["type"].as_str().unwrap(), "addRules");
    }
}
