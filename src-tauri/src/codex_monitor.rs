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

                    let session_id = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("codex")
                        .to_string();

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
                                crate::update_session_and_emit(&app, &state, &session_id, state_str, "monitor");
                                if let Some(entry) = state.lock().unwrap_or_else(|e| e.into_inner())
                                    .sessions.get_mut(&session_id)
                                {
                                    entry.agent_id = "Codex".into();
                                }
                            }
                        }
                    } else {
                        // Incremental: process each new line
                        for line in new_content.lines() {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                                if is_codex_session_end(&event) {
                                    crate::update_session_and_emit(&app, &state, &session_id, "idle", "SessionEnd");
                                    continue;
                                }

                                let event_type = event["type"].as_str().unwrap_or("").to_string();
                                if let Some(state_str) = map_codex_event(&event) {
                                    crate::update_session_and_emit(&app, &state, &session_id, state_str, &event_type);
                                    if let Some(entry) = state.lock().unwrap_or_else(|e| e.into_inner())
                                        .sessions.get_mut(&session_id)
                                    {
                                        if entry.agent_id.is_empty() {
                                            entry.agent_id = "Codex".into();
                                        }
                                    }
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

/// Map a Codex JSONL entry to a Clyde animation state.
///
/// Codex JSONL format (nested structure):
/// - `{type: "event_msg", payload: {type: "task_started"}}` → thinking
/// - `{type: "event_msg", payload: {type: "user_message"}}` → thinking
/// - `{type: "event_msg", payload: {type: "agent_message"}}` → idle
/// - `{type: "response_item", payload: {type: "function_call"}}` → working
/// - `{type: "response_item", payload: {type: "function_call_output"}}` → working
/// - `{type: "response_item", payload: {type: "message", role: "assistant"}}` → idle (if end_turn)
/// - `{type: "event_msg", payload: {type: "task_completed"}}` → session end
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

/// Find all .jsonl files in the Codex sessions directory.
/// Codex nests files under date subdirectories: sessions/YYYY/MM/DD/*.jsonl
fn find_codex_jsonl_files(base: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    // Walk up to 3 levels deep: year/month/day
    let years = match std::fs::read_dir(base) { Ok(e) => e, Err(_) => return files };
    for year in years.flatten() {
        let year_path = year.path();
        if !year_path.is_dir() { continue; }
        let months = match std::fs::read_dir(&year_path) { Ok(e) => e, Err(_) => continue };
        for month in months.flatten() {
            let month_path = month.path();
            if !month_path.is_dir() { continue; }
            let days = match std::fs::read_dir(&month_path) { Ok(e) => e, Err(_) => continue };
            for day in days.flatten() {
                let day_path = day.path();
                if !day_path.is_dir() { continue; }
                let entries = match std::fs::read_dir(&day_path) { Ok(e) => e, Err(_) => continue };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        files.push(path);
                    }
                }
            }
        }
    }
    // Also check for .jsonl files directly in base (legacy format)
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files
}
