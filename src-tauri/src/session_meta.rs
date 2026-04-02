use crate::state_machine::SharedState;
use crate::util::MutexExt;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ResolvedSessionMeta {
    pub summary: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionDisplayMeta {
    pub agent_label: String,
    pub summary: String,
    pub project: String,
    pub short_id: String,
}

pub(crate) fn display_agent_label(agent: &str) -> String {
    let normalized = agent.trim().to_ascii_lowercase();
    if normalized.contains("claude") {
        "Claude".into()
    } else if normalized.contains("codex") {
        "Codex".into()
    } else if normalized.contains("copilot") {
        "Copilot".into()
    } else if agent.trim().is_empty() {
        "Unknown".into()
    } else {
        agent.trim().into()
    }
}

pub(crate) fn short_session_id(session_id: &str) -> String {
    let stripped = session_id
        .strip_prefix("claude-monitor-")
        .or_else(|| session_id.strip_prefix("codex-"))
        .unwrap_or(session_id);
    let candidate = stripped
        .rsplit('-')
        .find(|part| part.len() >= 6)
        .unwrap_or(stripped);
    candidate.chars().take(6).collect()
}

pub(crate) fn project_name_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn clean_resume_summary(raw: &str) -> String {
    let mut fallback = String::new();

    for line in raw.lines() {
        let cleaned_line = clean_summary_line(line);
        if cleaned_line.is_empty() {
            continue;
        }
        if fallback.is_empty() {
            fallback = cleaned_line.clone();
        }
        if is_resume_noise(&cleaned_line) {
            continue;
        }
        return truncate_summary(&cleaned_line, 88);
    }

    truncate_summary(&fallback, 88)
}

pub(crate) fn ensure_session_display_meta(
    state: &SharedState,
    session_id: &str,
    fallback_agent: Option<&str>,
    fallback_cwd: Option<&str>,
) -> SessionDisplayMeta {
    let entry = {
        let sm = state.lock_or_recover();
        sm.sessions.get(session_id).cloned()
    };

    let mut agent_id = entry
        .as_ref()
        .map(|entry| entry.agent_id.trim().to_string())
        .filter(|agent| !agent.is_empty())
        .or_else(|| fallback_agent.map(|agent| agent.trim().to_string()))
        .unwrap_or_else(|| "claude-code".into());
    let mut summary = entry
        .as_ref()
        .map(|entry| entry.summary.clone())
        .unwrap_or_default();
    let mut cwd = entry
        .as_ref()
        .map(|entry| entry.cwd.clone())
        .unwrap_or_default();

    if summary.is_empty() || cwd.is_empty() {
        if let Some(resolved) = resolve_session_meta(session_id, &agent_id) {
            if summary.is_empty() && !resolved.summary.is_empty() {
                summary = resolved.summary.clone();
            }
            if cwd.is_empty() && !resolved.cwd.is_empty() {
                cwd = resolved.cwd.clone();
            }
            let mut sm = state.lock_or_recover();
            if let Some(entry) = sm.sessions.get_mut(session_id) {
                if entry.summary.is_empty() && !resolved.summary.is_empty() {
                    entry.summary = resolved.summary.clone();
                }
                if entry.cwd.is_empty() && !resolved.cwd.is_empty() {
                    entry.cwd = resolved.cwd.clone();
                }
                if entry.agent_id.trim().is_empty() {
                    entry.agent_id = agent_id.clone();
                }
            }
        }
    }

    if cwd.is_empty() {
        cwd = fallback_cwd.unwrap_or_default().to_string();
        if !cwd.is_empty() {
            let mut sm = state.lock_or_recover();
            if let Some(entry) = sm.sessions.get_mut(session_id) {
                if entry.cwd.is_empty() {
                    entry.cwd = cwd.clone();
                }
            }
        }
    }

    if agent_id.trim().is_empty() {
        agent_id = "claude-code".into();
    }

    SessionDisplayMeta {
        agent_label: display_agent_label(&agent_id),
        summary,
        project: project_name_from_cwd(&cwd),
        short_id: short_session_id(session_id),
    }
}

pub(crate) fn extract_tool_cwd(tool_input: &Value) -> Option<String> {
    const KEYS: &[&str] = &[
        "cwd",
        "workingDirectory",
        "working_directory",
        "dir",
        "path",
    ];
    KEYS.iter()
        .find_map(|key| tool_input.get(*key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_session_meta(session_id: &str, agent: &str) -> Option<ResolvedSessionMeta> {
    let normalized = agent.trim().to_ascii_lowercase();
    if normalized.contains("codex") {
        resolve_codex_session_meta(session_id)
    } else {
        resolve_claude_session_meta(session_id)
    }
}

fn resolve_claude_session_meta(session_id: &str) -> Option<ResolvedSessionMeta> {
    let base = dirs::home_dir()?.join(".claude").join("projects");
    let hint = session_id
        .strip_prefix("claude-monitor-")
        .unwrap_or(session_id);
    let path = find_claude_session_file(&base, hint)?;
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut meta = ResolvedSessionMeta::default();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if meta.cwd.is_empty() {
            meta.cwd = entry
                .get("cwd")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
        }
        if meta.summary.is_empty()
            && entry.get("type").and_then(|value| value.as_str()) == Some("user")
        {
            if let Some(content) = entry
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(extract_message_text)
            {
                meta.summary = clean_resume_summary(&content);
            }
        }
        if meta.summary.is_empty() {
            if let Some(last_prompt) = entry.get("lastPrompt").and_then(|value| value.as_str()) {
                meta.summary = clean_resume_summary(last_prompt);
            }
        }
        if !meta.summary.is_empty() && !meta.cwd.is_empty() {
            break;
        }
    }

    if meta.summary.is_empty() && meta.cwd.is_empty() {
        None
    } else {
        Some(meta)
    }
}

fn resolve_codex_session_meta(session_id: &str) -> Option<ResolvedSessionMeta> {
    let base = dirs::home_dir()?.join(".codex").join("sessions");
    let hint = session_id.strip_prefix("codex-").unwrap_or(session_id);
    let path = find_codex_session_file(&base, hint)?;
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut meta = ResolvedSessionMeta::default();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if meta.cwd.is_empty()
            && entry.get("type").and_then(|value| value.as_str()) == Some("session_meta")
        {
            meta.cwd = entry
                .get("payload")
                .and_then(|payload| payload.get("cwd"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
        }
        if meta.summary.is_empty()
            && entry.get("type").and_then(|value| value.as_str()) == Some("event_msg")
            && entry
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(|value| value.as_str())
                == Some("user_message")
        {
            if let Some(message) = entry
                .get("payload")
                .and_then(|payload| payload.get("message"))
                .and_then(|value| value.as_str())
            {
                meta.summary = clean_resume_summary(message);
            }
        }
        if meta.summary.is_empty()
            && entry.get("type").and_then(|value| value.as_str()) == Some("response_item")
            && entry
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(|value| value.as_str())
                == Some("message")
            && entry
                .get("payload")
                .and_then(|payload| payload.get("role"))
                .and_then(|value| value.as_str())
                == Some("user")
        {
            if let Some(content) = entry
                .get("payload")
                .and_then(|payload| payload.get("content"))
                .and_then(extract_message_text)
            {
                let cleaned = clean_resume_summary(&content);
                if !looks_like_system_prompt(&cleaned) {
                    meta.summary = cleaned;
                }
            }
        }
        if !meta.summary.is_empty() && !meta.cwd.is_empty() {
            break;
        }
    }

    if meta.summary.is_empty() && meta.cwd.is_empty() {
        None
    } else {
        Some(meta)
    }
}

fn extract_message_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    let parts = value.as_array()?;
    let mut chunks = Vec::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
            chunks.push(text.trim().to_string());
        }
    }
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

fn find_claude_session_file(base: &Path, hint: &str) -> Option<PathBuf> {
    let mut matches = Vec::new();
    let dirs = std::fs::read_dir(base).ok()?;
    for dir in dirs.flatten() {
        let path = dir.path();
        if !path.is_dir() {
            continue;
        }
        let files = match std::fs::read_dir(&path) {
            Ok(files) => files,
            Err(_) => continue,
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            if stem == hint || stem.starts_with(hint) || hint.starts_with(stem) {
                matches.push(path);
            }
        }
    }
    newest_path(matches)
}

fn find_codex_session_file(base: &Path, hint: &str) -> Option<PathBuf> {
    let mut stack = vec![base.to_path_buf()];
    let mut matches = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            if stem == hint || stem.contains(hint) {
                matches.push(path);
            }
        }
    }

    newest_path(matches)
}

fn newest_path(paths: Vec<PathBuf>) -> Option<PathBuf> {
    paths
        .into_iter()
        .max_by_key(|path| path.metadata().ok().and_then(|meta| meta.modified().ok()))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_summary_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_resume = trimmed
        .strip_prefix("/resume")
        .map(str::trim_start)
        .unwrap_or(trimmed);

    collapse_whitespace(without_resume)
}

fn is_resume_noise(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('#')
        || trimmed.starts_with('<')
        || trimmed.starts_with("```")
        || trimmed.starts_with("➜")
        || trimmed.starts_with('$')
        || trimmed.starts_with('%')
        || trimmed.starts_with('>')
        || trimmed.eq_ignore_ascii_case("user")
        || trimmed.eq_ignore_ascii_case("assistant")
        || trimmed.ends_with(".txt")
        || trimmed.ends_with(".json")
}

fn looks_like_system_prompt(text: &str) -> bool {
    text.starts_with("# AGENTS.md instructions")
        || text.starts_with("<environment_context>")
        || text.contains("<INSTRUCTIONS>")
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_resume_summary_from_shell_noise() {
        let raw = "➜ repo ls\nfoo.txt\n修复权限弹窗没有确认按钮的问题";
        assert_eq!(clean_resume_summary(raw), "修复权限弹窗没有确认按钮的问题");
    }

    #[test]
    fn strips_resume_prefix_from_inline_summary() {
        let raw = "/resume 修一下 permission request 的交互样式";
        assert_eq!(
            clean_resume_summary(raw),
            "修一下 permission request 的交互样式"
        );
    }

    #[test]
    fn skips_resume_only_line_and_uses_following_summary() {
        let raw = "/resume\n修一下 permission request 的交互样式";
        assert_eq!(
            clean_resume_summary(raw),
            "修一下 permission request 的交互样式"
        );
    }

    #[test]
    fn extracts_project_name_from_cwd() {
        assert_eq!(project_name_from_cwd("/Users/kinoko/Clyde"), "Clyde");
    }

    #[test]
    fn shortens_session_ids() {
        assert_eq!(short_session_id("claude-monitor-1f6449e7-6bcd"), "1f6449");
        assert_eq!(
            short_session_id("codex-rollout-2026-01-25-484ad236f061"),
            "484ad2"
        );
    }
}
