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
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

pub const CLYDE_SERVER_HEADER: &str = "x-clyde-server";
pub const CLYDE_SERVER_ID: &str = "clyde-on-desk";
pub const DEFAULT_PORT: u16 = 23333;
const REQUEST_WATCHDOG_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const REQUEST_DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const REQUEST_DEFAULT_DECISION_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(crate::prefs::DEFAULT_PERMISSION_DECISION_WINDOW_SECS as u64);

pub type PendingHookSender = oneshot::Sender<HookDecision>;
pub type PendingPerms = Arc<Mutex<HashMap<String, PendingHookSender>>>;
pub type ApprovalQueue = Arc<Mutex<ApprovalQueueState>>;

#[derive(Debug, Clone)]
pub enum PermDecision {
    Allow,
    Deny,
    AllowWithPermissions(Vec<serde_json::Value>),
}

#[derive(Debug, Clone)]
pub enum ElicitationDecision {
    Accept(Option<Value>),
    Decline,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum HookDecision {
    Permission(PermDecision),
    Elicitation(ElicitationDecision),
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

struct RequestDisplayMeta {
    session_id: String,
    agent_label: String,
    session_summary: String,
    session_project: String,
    session_short_id: String,
}

// NOTE: We accept raw JSON for permission requests because Claude Code's
// PermissionRequest hook payload format may vary. Fields are extracted manually.

fn payload_value<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| payload.get(*key))
}

fn payload_value_nested<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    if let Some(value) = payload_value(payload, keys) {
        return Some(value);
    }

    let nested_keys = [
        "request",
        "params",
        "input",
        "hook_input",
        "hookInput",
        "hookSpecificInput",
        "payload",
        "data",
        "elicitation",
        "body",
    ];

    nested_keys.iter().find_map(|key| {
        payload
            .get(*key)
            .and_then(|nested| payload_value_nested(nested, keys))
    })
}

fn payload_string(payload: &Value, keys: &[&str]) -> Option<String> {
    payload_value(payload, keys).and_then(value_to_string)
}

fn payload_string_nested(payload: &Value, keys: &[&str]) -> Option<String> {
    payload_value_nested(payload, keys).and_then(value_to_string)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(map) => {
            for key in [
                "text",
                "message",
                "content",
                "title",
                "prompt",
                "description",
            ] {
                if let Some(text) = map.get(key).and_then(value_to_string) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            let combined = items
                .iter()
                .filter_map(value_to_string)
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = combined.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    }
}

fn parse_json_string(value: &str) -> Option<Value> {
    serde_json::from_str::<Value>(value.trim()).ok()
}

fn schema_from_explicit_options(items: &[Value]) -> Option<Value> {
    let mut variants = Vec::new();

    for item in items {
        match item {
            Value::String(text) => variants.push(json!({
                "const": text,
                "title": text,
            })),
            Value::Number(number) => variants.push(json!({
                "const": number,
                "title": number.to_string(),
            })),
            Value::Bool(boolean) => variants.push(json!({
                "const": boolean,
                "title": boolean.to_string(),
            })),
            Value::Object(map) => {
                if map.contains_key("const") || map.contains_key("enum") {
                    variants.push(item.clone());
                    continue;
                }

                let value = map
                    .get("value")
                    .or_else(|| map.get("id"))
                    .or_else(|| map.get("key"))
                    .or_else(|| map.get("name"))
                    .or_else(|| map.get("choice"))
                    .cloned();
                let Some(value) = value else {
                    continue;
                };

                let mut variant = serde_json::Map::new();
                variant.insert("const".to_string(), value.clone());

                if let Some(title) = ["label", "title", "name", "text"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(value_to_string))
                {
                    variant.insert("title".to_string(), Value::String(title));
                } else if let Some(title) = value_to_string(&value) {
                    variant.insert("title".to_string(), Value::String(title));
                }

                if let Some(description) = ["description", "detail", "subtitle", "hint"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(value_to_string))
                {
                    variant.insert("description".to_string(), Value::String(description));
                }

                variants.push(Value::Object(variant));
            }
            _ => {}
        }
    }

    (!variants.is_empty()).then(|| json!({ "oneOf": variants }))
}

fn normalized_schema_value(value: &Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value.clone()),
        Value::String(text) => {
            let parsed = parse_json_string(text)?;
            normalized_schema_value(&parsed)
        }
        Value::Array(items) => schema_from_explicit_options(items),
        _ => None,
    }
}

fn extract_requested_schema(payload: &Value) -> Option<Value> {
    payload_value_nested(
        payload,
        &[
            "requested_schema",
            "requestedSchema",
            "schema",
            "input_schema",
            "inputSchema",
        ],
    )
    .and_then(normalized_schema_value)
    .or_else(|| {
        payload_value_nested(&payload, &["options", "choices", "responses", "items"])
            .and_then(|value| value.as_array())
            .and_then(|items| schema_from_explicit_options(items))
    })
}

fn extract_request_display_meta(
    ctx: &ServerCtx,
    payload: &Value,
    fallback_tool_input: &Value,
    default_agent: &str,
) -> RequestDisplayMeta {
    let session_id = payload_string_nested(payload, &["session_id", "sessionId"])
        .unwrap_or_else(|| "default".to_string());
    let agent_label_override = payload_string_nested(payload, &["agent_label", "agentLabel"]);
    let session_summary_override =
        payload_string_nested(payload, &["session_summary", "sessionSummary"]);
    let session_project_override =
        payload_string_nested(payload, &["session_project", "sessionProject"]);
    let session_short_id_override =
        payload_string_nested(payload, &["session_short_id", "sessionShortId"]);
    let fallback_agent = payload_string_nested(payload, &["agent_id", "agentId"])
        .unwrap_or_else(|| default_agent.to_string());
    let fallback_cwd = session_meta::extract_tool_cwd(fallback_tool_input);
    let display = session_meta::ensure_session_display_meta(
        &ctx.state,
        &session_id,
        Some(fallback_agent.as_str()),
        fallback_cwd.as_deref(),
    );

    let raw_session_summary = session_summary_override.unwrap_or(display.summary);

    RequestDisplayMeta {
        session_id,
        agent_label: agent_label_override.unwrap_or(display.agent_label),
        session_summary: session_meta::clean_resume_summary(&raw_session_summary),
        session_project: session_project_override.unwrap_or(display.project),
        session_short_id: session_short_id_override.unwrap_or(display.short_id),
    }
}

fn default_decision_for(bubble_data: &permission::BubbleData) -> HookDecision {
    if bubble_data.is_elicitation {
        HookDecision::Elicitation(ElicitationDecision::Cancel)
    } else {
        HookDecision::Permission(PermDecision::Deny)
    }
}

fn request_is_elicitation(approval_queue: &ApprovalQueue, id: &str) -> bool {
    approval_queue
        .lock_or_recover()
        .request_data
        .get(id)
        .map(|data| data.is_elicitation)
        .unwrap_or(false)
}

fn should_auto_resolve_request(
    session_advanced: bool,
    now: tokio::time::Instant,
    session_advance_grace_deadline: tokio::time::Instant,
    request_deadline: tokio::time::Instant,
) -> bool {
    now >= request_deadline || (session_advanced && now >= session_advance_grace_deadline)
}

fn request_decision_window(app: &AppHandle) -> Duration {
    app.try_state::<crate::prefs::SharedPrefs>()
        .map(|prefs: tauri::State<crate::prefs::SharedPrefs>| {
            let secs = crate::prefs::normalize_permission_decision_window_secs(
                prefs.lock_or_recover().permission_decision_window_secs,
            ) as u64;
            Duration::from_secs(secs)
        })
        .unwrap_or(REQUEST_DEFAULT_DECISION_WINDOW)
}

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

async fn queue_request_and_wait(
    ctx: &ServerCtx,
    bubble_data: permission::BubbleData,
) -> HookDecision {
    let entry_id = bubble_data.id.clone();
    let default_decision = default_decision_for(&bubble_data);
    let bubble_session_id = bubble_data.session_id.clone();
    let (tx, rx) = oneshot::channel::<HookDecision>();
    ctx.pending_perms
        .lock_or_recover()
        .insert(entry_id.clone(), tx);

    if !enqueue_permission_request(ctx, bubble_data) {
        return default_decision;
    }

    let watchdog_ctx = ctx.clone();
    let watchdog_id = entry_id.clone();
    let watchdog_default = default_decision.clone();
    let opened_at = std::time::Instant::now();
    let session_advance_grace_deadline =
        tokio::time::Instant::now() + request_decision_window(&ctx.app);
    let session_existed_at_open = {
        let sm = ctx.state.lock_or_recover();
        sm.sessions.contains_key(&bubble_session_id)
    };
    let watchdog = tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(REQUEST_WATCHDOG_POLL_INTERVAL);
        let request_deadline = tokio::time::Instant::now() + REQUEST_DEFAULT_TIMEOUT;
        loop {
            interval.tick().await;
            let still_pending = watchdog_ctx
                .pending_perms
                .lock_or_recover()
                .contains_key(&watchdog_id);
            if !still_pending {
                return;
            }
            let session_advanced = {
                let sm = watchdog_ctx.state.lock_or_recover();
                match sm.sessions.get(&bubble_session_id) {
                    Some(entry) => entry.updated_at > opened_at,
                    None => session_existed_at_open,
                }
            };
            if should_auto_resolve_request(
                session_advanced,
                tokio::time::Instant::now(),
                session_advance_grace_deadline,
                request_deadline,
            ) {
                break;
            }
        }

        if let Some(tx) = watchdog_ctx
            .pending_perms
            .lock_or_recover()
            .remove(&watchdog_id)
        {
            let _ = tx.send(watchdog_default);
        }
        close_permission_request_ui(
            &watchdog_ctx.app,
            &watchdog_ctx.pending_perms,
            &watchdog_ctx.approval_queue,
            &watchdog_ctx.bubble_map,
            &watchdog_id,
        );
    });

    let decision = rx.await.unwrap_or(default_decision);

    watchdog.abort();
    ctx.pending_perms.lock_or_recover().remove(&entry_id);
    close_permission_request_ui(
        &ctx.app,
        &ctx.pending_perms,
        &ctx.approval_queue,
        &ctx.bubble_map,
        &entry_id,
    );

    decision
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

    let tool_name = payload_string(&payload, &["tool_name", "toolName"])
        .unwrap_or_else(|| "unknown".to_string());
    let tool_input = payload_value(&payload, &["tool_input", "toolInput"])
        .cloned()
        .unwrap_or_else(|| json!({}));
    let suggestions = payload_value(
        &payload,
        &["permission_suggestions", "permissionSuggestions"],
    )
    .and_then(|value| value.as_array())
    .cloned()
    .unwrap_or_default();
    let display = extract_request_display_meta(&ctx, &payload, &tool_input, "claude-code");
    let entry_id = uuid::Uuid::new_v4().to_string();

    let bubble_data = permission::BubbleData {
        id: entry_id.clone(),
        window_kind: permission::WindowKind::ApprovalRequest,
        tool_name,
        tool_input,
        suggestions,
        session_id: display.session_id,
        agent_label: display.agent_label,
        session_summary: display.session_summary,
        session_project: display.session_project,
        session_short_id: display.session_short_id,
        is_elicitation: false,
        elicitation_message: None,
        elicitation_schema: None,
        elicitation_mode: None,
        elicitation_url: None,
        elicitation_server_name: None,
        mode_label: None,
        mode_description: None,
    };
    let response = match queue_request_and_wait(&ctx, bubble_data).await {
        HookDecision::Permission(decision) => perm_response(&decision),
        HookDecision::Elicitation(_) => perm_response(&PermDecision::Deny),
    };

    (StatusCode::OK, headers, Json(response))
}

async fn post_elicitation(
    AxumState(ctx): AxumState<ServerCtx>,
    Json(payload): Json<Value>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(CLYDE_SERVER_HEADER, CLYDE_SERVER_ID.parse().unwrap());

    eprintln!(
        "Clyde: /elicitation payload keys: {:?}",
        payload
            .as_object()
            .map(|o| o.keys().collect::<Vec<_>>())
            .unwrap_or_default()
    );

    let tool_input = payload_value_nested(&payload, &["tool_input", "toolInput", "input"])
        .cloned()
        .unwrap_or_else(|| json!({}));
    let display = extract_request_display_meta(&ctx, &payload, &tool_input, "claude-code");
    let elicitation_message = payload_string_nested(
        &payload,
        &["message", "prompt", "question", "title", "description"],
    )
    .or_else(|| {
        payload_value_nested(&payload, &["request", "params", "input"]).and_then(value_to_string)
    });
    let elicitation_schema = extract_requested_schema(&payload);
    let elicitation_mode =
        payload_string_nested(&payload, &["mode", "elicitation_mode", "elicitationMode"]);
    let elicitation_url = payload_string_nested(&payload, &["url", "href"]);
    let elicitation_server_name = payload_string_nested(
        &payload,
        &[
            "server_name",
            "serverName",
            "mcp_server_name",
            "mcpServerName",
        ],
    );
    let entry_id = uuid::Uuid::new_v4().to_string();

    let bubble_data = permission::BubbleData {
        id: entry_id,
        window_kind: permission::WindowKind::ApprovalRequest,
        tool_name: "Elicitation".to_string(),
        tool_input,
        suggestions: vec![],
        session_id: display.session_id,
        agent_label: display.agent_label,
        session_summary: display.session_summary,
        session_project: display.session_project,
        session_short_id: display.session_short_id,
        is_elicitation: true,
        elicitation_message,
        elicitation_schema,
        elicitation_mode,
        elicitation_url,
        elicitation_server_name,
        mode_label: None,
        mode_description: None,
    };

    let response = match queue_request_and_wait(&ctx, bubble_data).await {
        HookDecision::Elicitation(decision) => elicitation_response(&decision),
        HookDecision::Permission(_) => elicitation_response(&ElicitationDecision::Cancel),
    };

    (StatusCode::OK, headers, Json(response))
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
        let _ = tx.send(default_decision_for(&bubble_data));
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
    let default_decision = approval_queue
        .lock_or_recover()
        .request_data
        .get(id)
        .cloned()
        .map(|data| default_decision_for(&data))
        .unwrap_or(HookDecision::Permission(PermDecision::Deny));
    if let Some(tx) = pending_perms.lock_or_recover().remove(id) {
        let _ = tx.send(default_decision);
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

fn elicitation_response(decision: &ElicitationDecision) -> Value {
    let response = match decision {
        ElicitationDecision::Accept(Some(content)) => {
            json!({ "action": "accept", "content": content })
        }
        ElicitationDecision::Accept(None) => json!({ "action": "accept" }),
        ElicitationDecision::Decline => json!({ "action": "decline" }),
        ElicitationDecision::Cancel => json!({ "action": "cancel" }),
    };

    json!({
        "hookSpecificOutput": {
            "hookEventName": "Elicitation",
            "response": response
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
        .route("/elicitation", post(post_elicitation))
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
    elicitation_content: Option<serde_json::Value>,
) {
    let is_elicitation = request_is_elicitation(&approval_queue, &id);
    let tx = { pending.lock_or_recover().remove(&id) };
    if let Some(tx) = tx {
        let hook_decision = if is_elicitation {
            match decision.as_str() {
                "accept" | "allow" => {
                    HookDecision::Elicitation(ElicitationDecision::Accept(elicitation_content))
                }
                "decline" => HookDecision::Elicitation(ElicitationDecision::Decline),
                _ => HookDecision::Elicitation(ElicitationDecision::Cancel),
            }
        } else {
            match (decision.as_str(), selected_suggestion) {
                ("allow", Some(sug)) => {
                    HookDecision::Permission(PermDecision::AllowWithPermissions(vec![sug]))
                }
                ("allow", None) => HookDecision::Permission(PermDecision::Allow),
                _ => HookDecision::Permission(PermDecision::Deny),
            }
        };
        let _ = tx.send(hook_decision);
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

    #[test]
    fn test_elicitation_response_accept() {
        let resp = elicitation_response(&ElicitationDecision::Accept(Some(json!({
            "choice": "Option A"
        }))));
        let response = &resp["hookSpecificOutput"]["response"];
        assert_eq!(
            resp["hookSpecificOutput"]["hookEventName"]
                .as_str()
                .unwrap(),
            "Elicitation"
        );
        assert_eq!(response["action"].as_str().unwrap(), "accept");
        assert_eq!(response["content"]["choice"].as_str().unwrap(), "Option A");
    }

    #[test]
    fn test_elicitation_response_cancel() {
        let resp = elicitation_response(&ElicitationDecision::Cancel);
        let response = &resp["hookSpecificOutput"]["response"];
        assert_eq!(response["action"].as_str().unwrap(), "cancel");
        assert!(response.get("content").is_none());
    }

    #[test]
    fn test_extract_requested_schema_from_nested_options() {
        let payload = json!({
            "request": {
                "hookSpecificInput": {
                    "prompt": "Pick one",
                    "options": [
                        {
                            "value": "continue",
                            "label": "Continue",
                            "description": "Apply the plan"
                        },
                        {
                            "value": "revise",
                            "label": "Revise",
                            "description": "Make changes first"
                        }
                    ]
                }
            }
        });

        let schema = extract_requested_schema(&payload).expect("schema should be synthesized");
        let options = schema["oneOf"].as_array().expect("oneOf options");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0]["const"].as_str().unwrap(), "continue");
        assert_eq!(options[0]["title"].as_str().unwrap(), "Continue");
        assert_eq!(options[1]["const"].as_str().unwrap(), "revise");
    }

    #[test]
    fn test_extract_requested_schema_from_stringified_schema() {
        let payload = json!({
            "params": {
                "requested_schema": "{\"type\":\"string\",\"oneOf\":[{\"const\":\"a\",\"title\":\"Option A\"},{\"const\":\"b\",\"title\":\"Option B\"}]}"
            }
        });

        let schema = extract_requested_schema(&payload).expect("schema should parse from string");
        assert_eq!(schema["type"].as_str().unwrap(), "string");
        let options = schema["oneOf"].as_array().expect("oneOf options");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0]["title"].as_str().unwrap(), "Option A");
        assert_eq!(options[1]["const"].as_str().unwrap(), "b");
    }

    #[test]
    fn test_should_not_auto_resolve_before_min_decision_window() {
        let now = tokio::time::Instant::now();
        let grace_deadline = now + std::time::Duration::from_secs(12);
        let request_deadline = now + std::time::Duration::from_secs(300);

        assert!(!should_auto_resolve_request(
            true,
            now + std::time::Duration::from_secs(2),
            grace_deadline,
            request_deadline,
        ));
    }

    #[test]
    fn test_should_auto_resolve_after_min_decision_window_when_session_advanced() {
        let now = tokio::time::Instant::now();
        let grace_deadline = now + std::time::Duration::from_secs(12);
        let request_deadline = now + std::time::Duration::from_secs(300);

        assert!(should_auto_resolve_request(
            true,
            now + std::time::Duration::from_secs(12),
            grace_deadline,
            request_deadline,
        ));
    }
}
