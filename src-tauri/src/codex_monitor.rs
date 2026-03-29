use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, BufReader};
use std::path::PathBuf;
use std::time::Duration;
use tauri::AppHandle;
use crate::state_machine::SharedState;

const POLL_INTERVAL_MS: u64 = 1500;

/// Polls ~/.codex/sessions/*.jsonl for new events.
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

            loop {
                std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                if !codex_dir.exists() { continue; }

                let entries = match std::fs::read_dir(&codex_dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }

                    let file = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let file_len = match file.metadata() {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };
                    // Detect file truncation/rotation: if file shrank, restart from beginning
                    let stored_offset = known_files.get(&path).copied().unwrap_or(0);
                    let offset = if file_len < stored_offset { 0 } else { stored_offset };
                    if file_len <= offset { continue; }

                    let mut reader = BufReader::new(file);
                    if reader.seek(SeekFrom::Start(offset)).is_err() { continue; }
                    let mut new_content = String::new();
                    if reader.read_to_string(&mut new_content).is_err() { continue; }
                    let new_offset = file_len;
                    known_files.insert(path.clone(), new_offset);

                    for line in new_content.lines() {
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                            let session_id = path.file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("codex")
                                .to_string();
                            if let Some(state_str) = map_codex_event(&event) {
                                let event_type = event["type"].as_str().unwrap_or("").to_string();
                                crate::update_session_and_emit(&app, &state, &session_id, state_str, &event_type);
                            }
                        }
                    }
                }

                // Clean up entries for files that no longer exist
                known_files.retain(|path, _| path.exists());
            }
        });
}

fn map_codex_event(event: &serde_json::Value) -> Option<&'static str> {
    match event["type"].as_str()? {
        "user_message"      => Some("thinking"),
        "tool_call"         => Some("working"),
        "tool_response"     => Some("working"),
        "assistant_message" => Some("idle"),
        "session_end"       => Some("idle"),
        _ => None,
    }
}
