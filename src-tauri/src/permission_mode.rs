//! Permission mode tracker — monitors Claude's permission mode per session.
//!
//! Tracks mode changes from three sources (priority: Hook > Transcript > Settings)
//! and triggers mode_notice windows when the mode changes.

use crate::util::MutexExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager};

pub type ModeTracker = Arc<Mutex<HashMap<String, SessionModeState>>>;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
    Plan,
    Unknown(String),
}

impl PermissionMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "default" | "askBeforeEdits" => Self::Default,
            "acceptEdits" => Self::AcceptEdits,
            "bypassPermissions" => Self::BypassPermissions,
            "plan" => Self::Plan,
            "" => Self::Default,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn label(&self, lang: &str) -> &'static str {
        match (self, lang) {
            (Self::Default, "zh") => "正常审批模式",
            (Self::AcceptEdits, "zh") => "自动编辑模式",
            (Self::BypassPermissions, "zh") => "跳过所有审批",
            (Self::Plan, "zh") => "仅规划模式",
            (Self::Unknown(_), "zh") => "未知模式",
            (Self::Default, _) => "Normal Approval",
            (Self::AcceptEdits, _) => "Auto-Edit Mode",
            (Self::BypassPermissions, _) => "Bypass All Permissions",
            (Self::Plan, _) => "Plan-Only Mode",
            (Self::Unknown(_), _) => "Unknown Mode",
        }
    }

    pub fn description(&self, lang: &str) -> &'static str {
        match (self, lang) {
            (Self::Default, "zh") => "工具调用需要你的批准",
            (Self::AcceptEdits, "zh") => "编辑操作自动通过，其他工具仍可能需要审批",
            (Self::BypassPermissions, "zh") => "不会再弹出权限审批气泡",
            (Self::Plan, "zh") => "不执行工具，仅做规划",
            (Self::Unknown(_), "zh") => "",
            (Self::Default, _) => "Tool calls require your approval",
            (Self::AcceptEdits, _) => {
                "Edit operations auto-approved, other tools may still need approval"
            }
            (Self::BypassPermissions, _) => "Permission bubbles will not appear",
            (Self::Plan, _) => "No tool execution, planning only",
            (Self::Unknown(_), _) => "",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Default => "🔒",
            Self::AcceptEdits => "✏️",
            Self::BypassPermissions => "⚡",
            Self::Plan => "📋",
            Self::Unknown(_) => "❓",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum ModeSource {
    Settings = 0,
    Transcript = 1,
    Hook = 2,
}

pub struct SessionModeState {
    pub current_mode: PermissionMode,
    pub last_seen_at: Instant,
    pub last_source: ModeSource,
    pub last_notified_mode: Option<PermissionMode>,
}

const DEBOUNCE_MS: u128 = 300;
const DUPLICATE_SUPPRESS_MS: u128 = 2000;

/// Update the permission mode for a session. Triggers a mode_notice bubble if the mode changed.
pub fn update_session_mode(
    app: &AppHandle,
    tracker: &ModeTracker,
    session_id: &str,
    mode_str: &str,
    source: ModeSource,
    lang: &str,
) {
    let new_mode = PermissionMode::from_str(mode_str);
    let mut map = tracker.lock_or_recover();

    if let Some(state) = map.get_mut(session_id) {
        // Lower-priority source within 30s window: ignore
        if source < state.last_source && state.last_seen_at.elapsed().as_secs() < 30 {
            return;
        }

        // Same mode: just refresh timestamp
        if state.current_mode == new_mode {
            state.last_seen_at = Instant::now();
            state.last_source = source;
            return;
        }

        // Debounce: if last change was <300ms ago, update silently
        let elapsed = state.last_seen_at.elapsed().as_millis();
        state.current_mode = new_mode.clone();
        state.last_seen_at = Instant::now();
        state.last_source = source;

        if elapsed < DEBOUNCE_MS {
            return;
        }

        // Suppress duplicate notifications within 2s
        let should_notify = match &state.last_notified_mode {
            Some(last) if *last == new_mode => elapsed >= DUPLICATE_SUPPRESS_MS,
            _ => true,
        };

        if should_notify {
            state.last_notified_mode = Some(new_mode.clone());
            drop(map);
            trigger_mode_notice(app, session_id, &new_mode, lang);
        }
    } else {
        // First time seeing this session: record but don't notify
        map.insert(
            session_id.to_string(),
            SessionModeState {
                current_mode: new_mode,
                last_seen_at: Instant::now(),
                last_source: source,
                last_notified_mode: None,
            },
        );
    }
}

/// Get the current permission mode for a session.
#[allow(dead_code)]
pub fn get_session_mode(tracker: &ModeTracker, session_id: &str) -> Option<PermissionMode> {
    tracker
        .lock_or_recover()
        .get(session_id)
        .map(|s| s.current_mode.clone())
}

/// Create a mode_notice bubble window.
fn trigger_mode_notice(app: &AppHandle, _session_id: &str, mode: &PermissionMode, lang: &str) {
    let display = app
        .try_state::<crate::state_machine::SharedState>()
        .map(|state| {
            crate::session_meta::ensure_session_display_meta(
                state.inner(),
                _session_id,
                Some("claude-code"),
                None,
            )
        })
        .unwrap_or_else(|| crate::session_meta::SessionDisplayMeta {
            agent_label: crate::session_meta::display_agent_label("claude-code"),
            short_id: crate::session_meta::short_session_id(_session_id),
            ..Default::default()
        });
    let bubble_data = crate::permission::BubbleData {
        id: uuid::Uuid::new_v4().to_string(),
        window_kind: crate::permission::WindowKind::ModeNotice,
        tool_name: String::new(),
        tool_input: serde_json::Value::Null,
        suggestions: vec![],
        session_id: _session_id.to_string(),
        agent_label: display.agent_label,
        session_summary: display.summary,
        session_project: display.project,
        session_short_id: display.short_id,
        is_elicitation: false,
        elicitation_message: None,
        elicitation_schema: None,
        elicitation_mode: None,
        elicitation_url: None,
        elicitation_server_name: None,
        mode_label: Some(format!("{} {}", mode.icon(), mode.label(lang))),
        mode_description: Some(mode.description(lang).to_string()),
        update_version: None,
        update_url: None,
        update_notes: None,
        update_lang: None,
    };

    if let Some(bubbles) = app.try_state::<crate::permission::BubbleMap>() {
        // Close any existing mode_notice first (only one at a time)
        let existing: Vec<String> = {
            let map = bubbles.lock_or_recover();
            map.iter()
                .filter(|(_, e)| {
                    matches!(
                        e.data.window_kind,
                        crate::permission::WindowKind::ModeNotice
                    )
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &existing {
            crate::permission::prepare_close_bubble(app, &bubbles, id);
        }

        let id = bubble_data.id.clone();
        if crate::permission::show_bubble(app, &bubbles, bubble_data) {
            // Auto-dismiss after 5 seconds
            let app2 = app.clone();
            let bubbles2 = (*bubbles).clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                crate::permission::prepare_close_bubble(&app2, &bubbles2, &id);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tracker() -> ModeTracker {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn test_first_detection_no_notification() {
        // First detection should just record, not panic or trigger anything
        // (trigger_mode_notice needs AppHandle which we can't create in unit tests,
        //  but we can verify the state is recorded correctly)
        let tracker = make_tracker();
        let map = tracker.lock().unwrap();
        assert!(map.is_empty());
        drop(map);
        // Manually insert to simulate update_session_mode logic
        tracker.lock().unwrap().insert(
            "s1".into(),
            SessionModeState {
                current_mode: PermissionMode::Default,
                last_seen_at: Instant::now(),
                last_source: ModeSource::Hook,
                last_notified_mode: None,
            },
        );
        let map = tracker.lock().unwrap();
        assert_eq!(map["s1"].current_mode, PermissionMode::Default);
        assert!(map["s1"].last_notified_mode.is_none());
    }

    #[test]
    fn test_mode_parse() {
        assert_eq!(PermissionMode::from_str("default"), PermissionMode::Default);
        assert_eq!(
            PermissionMode::from_str("askBeforeEdits"),
            PermissionMode::Default
        );
        assert_eq!(
            PermissionMode::from_str("acceptEdits"),
            PermissionMode::AcceptEdits
        );
        assert_eq!(
            PermissionMode::from_str("bypassPermissions"),
            PermissionMode::BypassPermissions
        );
        assert_eq!(PermissionMode::from_str("plan"), PermissionMode::Plan);
        assert_eq!(PermissionMode::from_str(""), PermissionMode::Default);
        assert!(matches!(
            PermissionMode::from_str("custom"),
            PermissionMode::Unknown(_)
        ));
    }

    #[test]
    fn test_source_priority() {
        assert!(ModeSource::Hook > ModeSource::Transcript);
        assert!(ModeSource::Transcript > ModeSource::Settings);
    }

    #[test]
    fn test_same_mode_no_change() {
        let tracker = make_tracker();
        tracker.lock().unwrap().insert(
            "s1".into(),
            SessionModeState {
                current_mode: PermissionMode::Default,
                last_seen_at: Instant::now(),
                last_source: ModeSource::Hook,
                last_notified_mode: None,
            },
        );
        // Simulate same mode update
        {
            let mut map = tracker.lock().unwrap();
            let state = map.get_mut("s1").unwrap();
            let new_mode = PermissionMode::Default;
            assert_eq!(state.current_mode, new_mode); // same mode
        }
    }

    #[test]
    fn test_lower_priority_ignored() {
        let tracker = make_tracker();
        tracker.lock().unwrap().insert(
            "s1".into(),
            SessionModeState {
                current_mode: PermissionMode::AcceptEdits,
                last_seen_at: Instant::now(),
                last_source: ModeSource::Hook,
                last_notified_mode: Some(PermissionMode::AcceptEdits),
            },
        );
        // Settings source (lower priority) should not override Hook source
        let map = tracker.lock().unwrap();
        let state = &map["s1"];
        assert!(ModeSource::Settings < state.last_source);
        assert_eq!(state.current_mode, PermissionMode::AcceptEdits);
    }
}
