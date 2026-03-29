use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, BufReader};
use std::path::PathBuf;
use std::time::Duration;
use tauri::AppHandle;
use crate::state_machine::SharedState;

const POLL_INTERVAL_MS: u64 = 1500;

/// Polls ~/.codex/sessions/ for new events in JSONL files.
/// Codex stores sessions in nested date directories: sessions/YYYY/MM/DD/*.jsonl
/// Runs on a dedicated OS thread to avoid blocking the tokio runtime with
/// synchronous file I/O (read_dir, File::open, read_to_string).
pub fn start_codex_monitor(app: AppHandle, state: SharedState) {
    let _ = std::thread::Builder::new()
        .name("codex-monitor".into())
        .spawn(move || {
            let codex_dir = match dirs::home_dir() {
                Some(h) => h.join(".codex").join("sessions"),
                None => return,
            };
            let mut known_files: HashMap<PathBuf, u64> = HashMap::new();
            println!("Clyde: codex monitor started, watching {}", codex_dir.display());

            loop {
                std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                if !codex_dir.exists() { continue; }

                // Scan nested date directories: sessions/YYYY/MM/DD/*.jsonl
                let jsonl_files = find_codex_jsonl_files(&codex_dir);

                for path in jsonl_files {

                    let file = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let file_len = match file.metadata() {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };
                    // Detect file truncation/rotation: if file shrank, restart from beginning
                    let stored_offset = known_files.get(&path).copied();
                    let first_time = stored_offset.is_none();
                    let offset = match stored_offset {
                        Some(prev) if file_len < prev => 0, // file truncated, restart
                        Some(prev) => prev,
                        None => 0, // First time: read from start to detect current state
                    };
                    if file_len <= offset {
                        known_files.insert(path.clone(), file_len);
                        continue;
                    }

                    let mut reader = BufReader::new(file);
                    if reader.seek(SeekFrom::Start(offset)).is_err() { continue; }
                    let mut new_content = String::new();
                    if reader.read_to_string(&mut new_content).is_err() { continue; }
                    let new_offset = file_len;
                    known_files.insert(path.clone(), new_offset);

                    let session_id = format!("codex-{}", path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown"));

                    if first_time {
                        // First time seeing this file: only apply the last known state
                        // (avoids replaying entire session history as rapid state changes)
                        let mut last_state: Option<&str> = None;
                        let mut ended = false;
                        for line in new_content.lines() {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                                if is_codex_session_end(&event) { ended = true; }
                                if let Some(s) = map_codex_event(&event) { last_state = Some(s); }
                            }
                        }
                        if !ended {
                            if let Some(state_str) = last_state {
                                codex_update_and_emit(&app, &state, &session_id, state_str, "monitor");
                            }
                        }
                    } else {
                        // Incremental: process each new line
                        for line in new_content.lines() {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                                if is_codex_session_end(&event) {
                                    codex_update_and_emit(&app, &state, &session_id, "idle", "SessionEnd");
                                    continue;
                                }

                                let event_type = event["type"].as_str().unwrap_or("");
                                if let Some(state_str) = map_codex_event(&event) {
                                    codex_update_and_emit(&app, &state, &session_id, state_str, event_type);
                                }
                            }
                        }
                    }
                }

                // Clean up entries for files that no longer exist
                known_files.retain(|path, _| path.exists());
            }
        });
}

/// Update state machine and emit — same as `update_session_and_emit` but
/// atomically sets `agent_id = "Codex"` in the same lock to avoid the default
/// "claude-code" label from `SessionEntry::new()`.
fn codex_update_and_emit(app: &AppHandle, state: &SharedState, session_id: &str, state_str: &str, event: &str) {
    let (resolved, svg) = {
        let mut sm = state.lock().unwrap_or_else(|e| e.into_inner());
        if event == "SessionEnd" {
            sm.handle_session_end(session_id);
        } else {
            sm.update_session_state(session_id, state_str, event);
        }
        // Set agent_id atomically — before releasing the lock
        if let Some(entry) = sm.sessions.get_mut(session_id) {
            entry.agent_id = "Codex".into();
        }
        let resolved = sm.resolve_display_state();
        let svg = sm.svg_for_state(&resolved);
        sm.current_state = resolved.clone();
        sm.current_svg = svg.clone();
        (resolved, svg)
    };
    crate::emit_state(app, &resolved, &svg);
}

/// Map a Codex JSONL entry to a Clyde animation state.
///
/// Codex JSONL format (nested structure):
/// - `{type: "event_msg", payload: {type: "task_started"}}` → thinking
/// - `{type: "event_msg", payload: {type: "user_message"}}` → thinking
/// - `{type: "event_msg", payload: {type: "agent_message"}}` → idle
/// - `{type: "response_item", payload: {type: "function_call"}}` → working
/// - `{type: "response_item", payload: {type: "function_call_output"}}` → working
/// - `{type: "response_item", payload: {type: "message", role: "assistant"}}` → idle (if end_turn)
/// - `{type: "event_msg", payload: {type: "task_complete"}}` → session end
fn map_codex_event(event: &serde_json::Value) -> Option<&'static str> {
    let top_type = event["type"].as_str()?;

    match top_type {
        "event_msg" => {
            let inner_type = event["payload"]["type"].as_str()?;
            match inner_type {
                "task_started" | "user_message" => Some("thinking"),
                "agent_message" => Some("idle"),
                // task_completed/task_cancelled handled separately as session end
                _ => None,
            }
        }
        "response_item" => {
            let payload_type = event["payload"]["type"].as_str()?;
            match payload_type {
                "function_call" => Some("working"),
                "function_call_output" => Some("working"),
                "reasoning" => Some("thinking"),
                "message" => {
                    let role = event["payload"]["role"].as_str().unwrap_or("");
                    if role == "assistant" {
                        // Check if this is a final response (has output_text content)
                        if let Some(content) = event["payload"]["content"].as_array() {
                            if content.iter().any(|c| c["type"].as_str() == Some("output_text")) {
                                return Some("idle");
                            }
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Check if a Codex event signals session end.
fn is_codex_session_end(event: &serde_json::Value) -> bool {
    event["type"].as_str() == Some("event_msg")
        && matches!(
            event["payload"]["type"].as_str(),
            Some("task_complete") | Some("task_cancelled")
        )
}

/// Maximum age (seconds) for a session file to be considered active.
const ACTIVE_SESSION_MAX_AGE_SECS: u64 = 3600; // 1 hour

/// Find active .jsonl files in the Codex sessions directory.
/// Codex nests files under date subdirectories: sessions/YYYY/MM/DD/*.jsonl
/// Only files modified within the last hour are considered active.
fn find_codex_jsonl_files(base: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_recursive(base, &mut files);
    // Filter to only files modified within the last hour
    files.retain(|path| {
        let age = path.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        age <= ACTIVE_SESSION_MAX_AGE_SECS
    });
    files
}

/// Recursively collect .jsonl files from a directory tree.
fn collect_jsonl_recursive(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_recursive(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}
