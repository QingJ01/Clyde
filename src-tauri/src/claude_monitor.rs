//! Claude Code session JSONL monitor.
//!
//! Polls `~/.claude/projects/` for `.jsonl` session files, similar to `codex_monitor.rs`.
//! This works for ALL Claude Code environments (CLI, VS Code extension, Claude Desktop)
//! because all of them write session JSONL files regardless of hook configuration.
//!
//! Falls back gracefully: if command hooks are also firing, the state machine's
//! priority resolution ensures the highest-priority state wins.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use crate::state_machine::SharedState;

const POLL_INTERVAL_MS: u64 = 2000;

pub fn start_claude_monitor(app: AppHandle, state: SharedState) {
    tauri::async_runtime::spawn(async move {
        let claude_dir = match dirs::home_dir() {
            Some(h) => h.join(".claude").join("projects"),
            None => return,
        };
        let mut known_files: HashMap<PathBuf, u64> = HashMap::new();
        let mut interval = tokio::time::interval(Duration::from_millis(POLL_INTERVAL_MS));

        loop {
            interval.tick().await;
            if !claude_dir.exists() { continue; }

            // Scan for .jsonl files (session logs)
            let jsonl_files = find_jsonl_files(&claude_dir);

            for path in jsonl_files {
                let offset = known_files.get(&path).copied().unwrap_or(0);
                let file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let file_len = match file.metadata() {
                    Ok(m) => m.len(),
                    Err(_) => continue,
                };
                if file_len <= offset { continue; }

                // First time seeing this file: skip to end (only process new lines)
                if offset == 0 && file_len > 0 {
                    known_files.insert(path.clone(), file_len);
                    continue;
                }

                let mut reader = BufReader::new(file);
                if reader.seek(SeekFrom::Start(offset)).is_err() { continue; }

                let mut last_state: Option<&'static str> = None;
                let mut session_id = extract_session_id(&path);

                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {},
                        Err(_) => break,
                    }
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }

                    if let Ok(entry) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        // Extract session_id from the entry if available
                        if let Some(sid) = entry.get("sessionId").and_then(|s| s.as_str()) {
                            session_id = format!("claude-monitor-{}", &sid[..sid.len().min(12)]);
                        }

                        // Extract permissionMode from transcript entries
                        if let Some(mode) = entry.get("permissionMode").and_then(|m| m.as_str()) {
                            if let Some(tracker) = app.try_state::<crate::permission_mode::ModeTracker>() {
                                let lang = app.try_state::<crate::prefs::SharedPrefs>()
                                    .map(|p: tauri::State<crate::prefs::SharedPrefs>| p.lock().unwrap_or_else(|e| e.into_inner()).lang.clone())
                                    .unwrap_or_else(|| "en".into());
                                crate::permission_mode::update_session_mode(
                                    &app, &tracker, &session_id, mode,
                                    crate::permission_mode::ModeSource::Transcript, &lang,
                                );
                            }
                        }

                        if let Some(state_str) = map_claude_event(&entry) {
                            last_state = Some(state_str);
                        }
                    }
                }

                known_files.insert(path.clone(), file_len);

                // Apply the last state from this batch of new lines
                if let Some(state_str) = last_state {
                    let (resolved, svg) = {
                        let mut sm = state.lock().unwrap_or_else(|e| e.into_inner());
                        // Only update if not already tracked by command hooks
                        // (hooks use session_id from Claude Code, monitor uses prefixed ID)
                        sm.update_session_state(&session_id, state_str, "monitor");
                        let resolved = sm.resolve_display_state();
                        let svg = sm.svg_for_state(&resolved);
                        sm.current_state = resolved.clone();
                        sm.current_svg = svg.clone();
                        (resolved, svg)
                    };
                    let _ = app.emit("state-change", serde_json::json!({
                        "state": resolved, "svg": svg
                    }));
                }
            }
        }
    });
}

/// Find all .jsonl files in the claude projects directory (non-recursive into subagents too).
fn find_jsonl_files(base: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return files,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Look inside project directories for .jsonl files
            if let Ok(inner) = std::fs::read_dir(&path) {
                for inner_entry in inner.flatten() {
                    let inner_path = inner_entry.path();
                    if inner_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        files.push(inner_path);
                    }
                }
            }
        }
    }
    files
}

/// Extract a session identifier from the file path.
fn extract_session_id(path: &PathBuf) -> String {
    let stem = path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("claude");
    format!("claude-monitor-{}", &stem[..stem.len().min(12)])
}

/// Map a Claude Code JSONL entry to a Clyde animation state.
fn map_claude_event(entry: &serde_json::Value) -> Option<&'static str> {
    let message = entry.get("message")?;
    let role = message.get("role")?.as_str()?;

    match role {
        "user" => Some("thinking"),
        "assistant" => {
            // Check content type for more granular state
            let content = message.get("content")?.as_array()?;
            let last = content.last()?;
            let content_type = last.get("type")?.as_str()?;
            match content_type {
                "thinking" => Some("thinking"),
                "tool_use" => Some("working"),
                "tool_result" => Some("working"),
                "text" => {
                    // Check stop_reason: if "end_turn" → idle, otherwise still working
                    let stop = message.get("stop_reason").and_then(|s| s.as_str());
                    match stop {
                        Some("end_turn") | Some("stop_sequence") => Some("idle"),
                        _ => None, // partial message, don't change state
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}
