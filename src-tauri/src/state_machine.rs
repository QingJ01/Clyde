use std::collections::HashMap;
use std::time::Instant;
use std::sync::{Arc, Mutex};

pub type SharedState = Arc<Mutex<StateMachine>>;

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub state: String,
    pub updated_at: Instant,
    pub source_pid: Option<u32>,
    pub cwd: String,
    pub agent_id: String,
}

impl SessionEntry {
    pub fn new(state: &str) -> Self {
        SessionEntry {
            state: state.to_string(),
            updated_at: Instant::now(),
            source_pid: None,
            cwd: String::new(),
            agent_id: "claude-code".into(),
        }
    }
}

/// Priority of each state for display resolution.
/// NOTE: Event-to-state mappings are defined in multiple locations:
///   1. hooks/clyde-hook.js    — EVENT_TO_STATE (Claude Code hook events → states)
///   2. hooks/copilot-hook.js  — EVENT_TO_STATE (Copilot hook events → states)
///   3. state_machine.rs       — state_priority() + svg_for_state() (state → priority/SVG)
///   4. codex_monitor.rs       — map_codex_event() (Codex JSONL events → states)
///   5. claude_monitor.rs      — map_claude_event() (Claude session events → states)
/// When adding a new state, update ALL locations above.
pub fn state_priority(s: &str) -> u8 {
    match s {
        "error"        => 8,
        "notification" => 7,
        "sweeping"     => 6,
        "attention"    => 5,
        "carrying"     => 4,
        "juggling"     => 4,
        "working"      => 3,
        "thinking"     => 2,
        "idle"         => 1,
        "sleeping"     => 0,
        _              => 0,
    }
}

pub const ONESHOT_STATES: &[&str] = &["attention", "error", "notification", "sweeping", "carrying"];
const SESSION_STALE_SECS: u64 = 600;
const WORKING_STALE_SECS: u64 = 300;

pub struct StateMachine {
    pub current_state: String,
    pub current_svg:   String,
    pub sessions:      HashMap<String, SessionEntry>,
    pub dnd:           bool,
}

impl StateMachine {
    pub fn new() -> Self {
        StateMachine {
            current_state: "idle".into(),
            current_svg:   "clyde-idle-follow.svg".into(),
            sessions:      HashMap::new(),
            dnd:           false,
        }
    }

    pub fn resolve_display_state(&self) -> String {
        if self.sessions.is_empty() { return "idle".into(); }
        let mut best = "sleeping";
        for s in self.sessions.values() {
            if state_priority(&s.state) > state_priority(best) {
                best = &s.state;
            }
        }
        best.to_string()
    }

    pub fn handle_session_end(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    pub fn update_session_state(&mut self, session_id: &str, state: &str, event: &str) {
        // Oneshot states: play animation but don't persist as session state.
        if ONESHOT_STATES.contains(&state) {
            if let Some(entry) = self.sessions.get_mut(session_id) {
                entry.updated_at = Instant::now();
                // "attention" (Stop/PostCompact) means task finished → reset to idle.
                // Other oneshots (error, notification, sweeping, carrying) are transient
                // — keep the session's current state so it resumes correctly.
                if state == "attention" {
                    entry.state = "idle".into();
                }
            }
            return;
        }
        let entry = self.sessions.entry(session_id.to_string())
            .or_insert_with(|| SessionEntry::new(state));
        // Protect juggling: don't downgrade to working unless SubagentStop
        if entry.state == "juggling" && state == "working"
            && event != "SubagentStop" && event != "subagentStop" {
            entry.updated_at = Instant::now();
            return;
        }
        entry.state      = state.to_string();
        entry.updated_at = Instant::now();
    }

    pub fn clean_stale(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let age = {
                let s = &self.sessions[&id];
                now.duration_since(s.updated_at).as_secs()
            };
            if age > SESSION_STALE_SECS {
                self.sessions.remove(&id);
                changed = true;
            } else if age > WORKING_STALE_SECS {
                if let Some(s) = self.sessions.get_mut(&id) {
                    if matches!(s.state.as_str(), "working" | "juggling" | "thinking") {
                        s.state = "idle".into();
                        s.updated_at = now;
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// Returns true if any sessions are currently tracked.
    #[allow(dead_code)]
    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    /// Session summaries for context menu display.
    pub fn session_summaries(&self) -> Vec<(String, String, Option<u32>, String)> {
        self.sessions.iter().map(|(id, e)| {
            (id.clone(), e.state.clone(), e.source_pid, e.agent_id.clone())
        }).collect()
    }

    pub fn svg_for_state(&self, state: &str) -> String {
        let working_count = self.sessions.values()
            .filter(|s| matches!(s.state.as_str(), "working" | "thinking" | "juggling"))
            .count();
        match state {
            "idle"         => "clyde-idle-follow.svg".into(),
            "working"      => match working_count {
                n if n >= 3 => "clyde-working-building.svg".into(),
                n if n >= 2 => "clyde-working-juggling.svg".into(),
                _           => "clyde-working-typing.svg".into(),
            },
            "juggling"     => {
                let n = self.sessions.values().filter(|s| s.state == "juggling").count();
                if n >= 2 { "clyde-working-conducting.svg".into() }
                else { "clyde-working-juggling.svg".into() }
            },
            "thinking"     => "clyde-working-thinking.svg".into(),
            "sweeping"     => "clyde-working-sweeping.svg".into(),
            "error"        => "clyde-error.svg".into(),
            "attention"    => "clyde-happy.svg".into(),
            "notification" => "clyde-notification.svg".into(),
            "carrying"     => "clyde-working-carrying.svg".into(),
            "sleeping"     => "clyde-sleeping.svg".into(),
            "yawning"      => "clyde-idle-yawn.svg".into(),
            "dozing"       => "clyde-idle-doze.svg".into(),
            "collapsing"   => "clyde-collapse-sleep.svg".into(),
            "waking"       => "clyde-wake.svg".into(),
            // Mini mode states
            "mini-idle"    => "clyde-mini-idle.svg".into(),
            "mini-alert"   => "clyde-mini-alert.svg".into(),
            "mini-happy"   => "clyde-mini-happy.svg".into(),
            "mini-peek"    => "clyde-mini-peek.svg".into(),
            "mini-enter"   => "clyde-mini-enter.svg".into(),
            "mini-sleep"   => "clyde-mini-sleep.svg".into(),
            _              => "clyde-idle-follow.svg".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_sessions_resolve_to_idle() {
        let sm = StateMachine::new();
        assert_eq!(sm.resolve_display_state(), "idle");
    }

    #[test]
    fn test_higher_priority_wins() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("a".into(), SessionEntry::new("thinking"));
        sm.sessions.insert("b".into(), SessionEntry::new("error"));
        assert_eq!(sm.resolve_display_state(), "error");
    }

    #[test]
    fn test_session_end_removes_entry() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("s1".into(), SessionEntry::new("working"));
        sm.handle_session_end("s1");
        assert_eq!(sm.sessions.len(), 0);
    }

    #[test]
    fn test_attention_resets_session_to_idle() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("s1".into(), SessionEntry::new("working"));
        sm.update_session_state("s1", "attention", "Stop");
        let s = sm.sessions.get("s1").unwrap();
        assert_eq!(s.state, "idle", "attention (Stop) should reset session state to idle");
    }

    #[test]
    fn test_notification_preserves_session_state() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("s1".into(), SessionEntry::new("working"));
        sm.update_session_state("s1", "notification", "Notification");
        let s = sm.sessions.get("s1").unwrap();
        assert_eq!(s.state, "working", "notification should not change session state");
    }

    #[test]
    fn test_error_preserves_session_state() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("s1".into(), SessionEntry::new("working"));
        sm.update_session_state("s1", "error", "PostToolUseFailure");
        let s = sm.sessions.get("s1").unwrap();
        assert_eq!(s.state, "working", "error is oneshot — session stays working");
    }

    #[test]
    fn test_priority_order() {
        assert!(state_priority("error") > state_priority("notification"));
        assert!(state_priority("notification") > state_priority("sweeping"));
        assert!(state_priority("working") > state_priority("thinking"));
        assert!(state_priority("thinking") > state_priority("idle"));
        assert!(state_priority("idle") > state_priority("sleeping"));
    }

    #[test]
    fn test_juggling_not_downgraded_by_working() {
        let mut sm = StateMachine::new();
        sm.sessions.insert("s1".into(), SessionEntry::new("juggling"));
        sm.update_session_state("s1", "working", "PreToolUse"); // not SubagentStop
        let s = sm.sessions.get("s1").unwrap();
        assert_eq!(s.state, "juggling", "juggling should not be downgraded to working");
    }

    #[test]
    fn test_svg_for_working_states() {
        let sm = StateMachine::new();
        assert_eq!(sm.svg_for_state("idle"),     "clyde-idle-follow.svg");
        assert_eq!(sm.svg_for_state("thinking"), "clyde-working-thinking.svg");
        assert_eq!(sm.svg_for_state("error"),    "clyde-error.svg");
        assert_eq!(sm.svg_for_state("sleeping"), "clyde-sleeping.svg");
    }
}
