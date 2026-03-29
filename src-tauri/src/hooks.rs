use anyhow::{Context, Result};
use std::path::PathBuf;

const HOOK_SCRIPT_NAME: &str = "clyde-hook.js";
const SERVER_CONFIG_NAME: &str = "server-config.js";
const COPILOT_HOOK_NAME: &str = "copilot-hook.js";
const COPILOT_HOOK_CONTENT: &str = include_str!("../../hooks/copilot-hook.js");
const AUTO_START_NAME: &str = "auto-start.js";
const AUTO_START_CONTENT: &str = include_str!("../../hooks/auto-start.js");

// Embed hook scripts at compile time — works in both dev and production
const HOOK_SCRIPT_CONTENT: &str = include_str!("../../hooks/clyde-hook.js");
const SERVER_CONFIG_CONTENT: &str = include_str!("../../hooks/server-config.js");

pub struct HookInstaller {
    /// Override for the settings.json path. None = default ~/.claude/settings.json.
    pub settings_path: Option<PathBuf>,
    /// HTTP server port for permission hook URL. None = skip permission hook registration.
    pub server_port: Option<u16>,
}

impl HookInstaller {
    pub fn register(&self) -> Result<()> {
        // Step 1: Deploy hook scripts to ~/.claude/hooks/
        let hook_dir = hook_dir()?;
        std::fs::create_dir_all(&hook_dir)
            .context("creating hooks directory")?;

        let hook_path = hook_dir.join(HOOK_SCRIPT_NAME);
        std::fs::write(&hook_path, HOOK_SCRIPT_CONTENT)
            .context("writing clyde-hook.js")?;

        let config_path = hook_dir.join(SERVER_CONFIG_NAME);
        std::fs::write(&config_path, SERVER_CONFIG_CONTENT)
            .context("writing server-config.js")?;

        let auto_start_path = hook_dir.join(AUTO_START_NAME);
        std::fs::write(&auto_start_path, AUTO_START_CONTENT)
            .context("writing auto-start.js")?;

        // Clean up legacy "clawd" files from previous versions
        let _ = std::fs::remove_file(hook_dir.join("clawd-hook.js"));

        // Step 2: Register hooks in settings.json
        let default_path = claude_settings_path()?;
        let settings_path = self.settings_path.as_deref().unwrap_or(&default_path);

        let mut settings: serde_json::Value = if settings_path.exists() {
            let raw = std::fs::read_to_string(settings_path)
                .context("reading settings.json")?;
            match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    // Backup before overwriting — don't silently destroy user config
                    let backup = settings_path.with_extension("json.bak");
                    let _ = std::fs::copy(settings_path, &backup);
                    eprintln!("Clyde: settings.json parse error ({e}), backed up to {}", backup.display());
                    serde_json::json!({})
                }
            }
        } else {
            serde_json::json!({})
        };

        const CORE_HOOKS: &[&str] = &[
            "SessionStart", "SessionEnd", "UserPromptSubmit",
            "PreToolUse", "PostToolUse", "PostToolUseFailure",
            "Stop", "SubagentStart", "SubagentStop",
            "Notification", "Elicitation", "WorktreeCreate",
        ];

        let obj = settings.as_object_mut()
            .context("settings.json must be a JSON object")?;
        let hooks = obj.entry("hooks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("hooks must be an object")?;

        let auto_start_cmd = format!("node \"{}\"", auto_start_path.display());

        for event in CORE_HOOKS {
            let hook_cmd = format!("node \"{}\" {}", hook_path.display(), event);

            let arr = hooks.entry(*event)
                .or_insert_with(|| serde_json::json!([]));
            if let Some(list) = arr.as_array_mut() {
                // Remove old entries (both new flat format and old nested format).
                // Match on exact filenames to avoid removing unrelated user hooks
                // that happen to contain "clyde" or "clawd" as a substring.
                const OUR_FILES: &[&str] = &[
                    HOOK_SCRIPT_NAME, AUTO_START_NAME, SERVER_CONFIG_NAME,
                    COPILOT_HOOK_NAME, "clawd-hook.js",
                ];
                let is_our_cmd = |cmd: &str| -> bool {
                    OUR_FILES.iter().any(|name| {
                        cmd.contains(&format!("/{name}"))
                            || cmd.contains(&format!("\\{name}"))
                            || cmd.ends_with(name)
                    })
                };
                list.retain(|v| {
                    if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                        if is_our_cmd(cmd) { return false; }
                    }
                    if let Some(hooks_arr) = v.get("hooks").and_then(|h| h.as_array()) {
                        for hook in hooks_arr {
                            if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                                if is_our_cmd(cmd) { return false; }
                            }
                        }
                    }
                    true
                });
                // For SessionStart, insert auto-start before the main hook
                if *event == "SessionStart" {
                    list.push(serde_json::json!({
                        "type":    "command",
                        "command": auto_start_cmd,
                    }));
                }
                list.push(serde_json::json!({
                    "type":    "command",
                    "command": hook_cmd,
                }));
            }
        }

        // Step 3: Register permission HTTP hook under PermissionRequest event.
        // Claude Code sends a blocking HTTP POST to this URL when requesting tool permission.
        // Response format: { hookSpecificOutput: { hookEventName, decision: { behavior } } }
        if let Some(port) = self.server_port {
            let perm_url = format!("http://127.0.0.1:{port}/permission");
            let arr = hooks.entry("PermissionRequest")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(list) = arr.as_array_mut() {
                // Remove any old Clyde permission hook entries
                list.retain(|v| {
                    // Check nested hooks array for our URL
                    if let Some(inner) = v.get("hooks").and_then(|h| h.as_array()) {
                        for hook in inner {
                            if let Some(url) = hook.get("url").and_then(|u| u.as_str()) {
                                if url.contains("/permission") { return false; }
                            }
                        }
                    }
                    // Also check flat format in case of old registration
                    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
                        if url.contains("/permission") { return false; }
                    }
                    true
                });
                list.push(serde_json::json!({
                    "matcher": "",
                    "hooks": [{
                        "type": "http",
                        "url":  perm_url,
                        "timeout": 600,
                    }]
                }));
            }
        }

        // Step 3b: Clean up stray entries in other events (e.g. old PreToolUse permission hook)
        for event in ["PreToolUse", "PostToolUse"] {
            if let Some(arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
                arr.retain(|v| {
                    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
                        if url.contains("/permission") { return false; }
                    }
                    true
                });
            }
        }

        // Step 4: Deploy Copilot hook script (user still needs to configure ~/.copilot/hooks/hooks.json)
        let copilot_path = hook_dir.join(COPILOT_HOOK_NAME);
        std::fs::write(&copilot_path, COPILOT_HOOK_CONTENT)
            .context("writing copilot-hook.js")?;

        // Step 4: Auto-configure Copilot hooks.json if ~/.copilot exists
        if let Ok(home) = dirs::home_dir().context("home dir") {
            let copilot_dir = home.join(".copilot").join("hooks");
            if copilot_dir.parent().map(|p| p.exists()).unwrap_or(false) {
                let _ = std::fs::create_dir_all(&copilot_dir);
                let hooks_json = copilot_dir.join("hooks.json");
                let copilot_cmd = format!("node \"{}\"", copilot_path.display());
                let config = serde_json::json!({
                    "hooks": {
                        "*": [{ "type": "command", "command": copilot_cmd }]
                    }
                });
                // Only write if not already configured
                let should_write = if hooks_json.exists() {
                    let raw = std::fs::read_to_string(&hooks_json).unwrap_or_default();
                    !raw.contains("copilot-hook.js") && !raw.contains("clyde")
                } else {
                    true
                };
                if should_write {
                    let _ = std::fs::write(&hooks_json, serde_json::to_string_pretty(&config).unwrap_or_default());
                }
            }
        }

        // Atomic write: temp file → rename
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = settings_path.with_extension("json.clyde-tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&settings)?)?;
        std::fs::rename(&tmp, settings_path)?;
        Ok(())
    }
}

fn claude_settings_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn hook_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".claude").join("hooks"))
}

/// Check if the PermissionRequest hook in settings.json is correctly formatted.
pub fn permission_hook_is_healthy(settings: &serde_json::Value, expected_url: &str) -> bool {
    let perm_arr = match settings.get("hooks")
        .and_then(|h| h.get("PermissionRequest"))
        .and_then(|p| p.as_array()) {
        Some(arr) => arr,
        None => return false,
    };
    for entry in perm_arr {
        // Must be nested format with matcher + hooks array
        if entry.get("matcher").is_none() { continue; }
        if let Some(inner) = entry.get("hooks").and_then(|h| h.as_array()) {
            for hook in inner {
                if hook.get("type").and_then(|t| t.as_str()) == Some("http") {
                    if let Some(url) = hook.get("url").and_then(|u| u.as_str()) {
                        if url == expected_url { return true; }
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_adds_hook_entry() {
        let tmp_file = std::env::temp_dir().join("clyde-test-settings-add.json");
        std::fs::write(&tmp_file, "{}").unwrap();

        let installer = HookInstaller {
            settings_path: Some(tmp_file.clone()),
            server_port: Some(23333),
        };
        installer.register().expect("register should succeed");

        let contents = std::fs::read_to_string(&tmp_file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let arr = parsed["hooks"]["SessionStart"].as_array().unwrap();
        // SessionStart has 2 entries: auto-start + main hook
        assert_eq!(arr.len(), 2, "SessionStart should have auto-start and main hook");
        let auto_cmd = arr[0]["command"].as_str().unwrap();
        assert!(auto_cmd.contains(AUTO_START_NAME), "first entry should be auto-start");
        let main_cmd = arr[1]["command"].as_str().unwrap();
        assert!(main_cmd.contains(HOOK_SCRIPT_NAME), "second entry should be main hook");
        assert!(main_cmd.contains("SessionStart"), "hook command should include event name");

        // PermissionRequest should have a nested HTTP hook entry with matcher
        let perm_arr = parsed["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_arr.len(), 1, "PermissionRequest should have one entry");
        let perm_entry = &perm_arr[0];
        assert_eq!(perm_entry["matcher"].as_str().unwrap(), "", "matcher should be empty (match all tools)");
        let inner_hooks = perm_entry["hooks"].as_array().unwrap();
        assert_eq!(inner_hooks.len(), 1);
        assert_eq!(inner_hooks[0]["type"].as_str().unwrap(), "http");
        assert!(inner_hooks[0]["url"].as_str().unwrap().contains("/permission"));

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn test_no_duplicate_registration() {
        let tmp_file = std::env::temp_dir().join("clyde-test-settings-dedup.json");
        std::fs::write(&tmp_file, "{}").unwrap();

        let installer = HookInstaller {
            settings_path: Some(tmp_file.clone()),
            server_port: Some(23333),
        };

        installer.register().expect("first register should succeed");
        installer.register().expect("second register should succeed");

        let contents = std::fs::read_to_string(&tmp_file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let arr = parsed["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "should not register hook twice (auto-start + main)");

        // PermissionRequest should not duplicate
        let perm_arr = parsed["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_arr.len(), 1, "PermissionRequest should not duplicate");

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn test_permission_request_flat_entry_is_rewritten() {
        let tmp_file = std::env::temp_dir().join("clyde-test-settings-flat-perm.json");
        // Seed with OLD flat PermissionRequest format
        let old_settings = serde_json::json!({
            "hooks": {
                "PermissionRequest": [
                    { "type": "http", "url": "http://127.0.0.1:23333/permission", "timeout": 600 }
                ]
            }
        });
        std::fs::write(&tmp_file, serde_json::to_string_pretty(&old_settings).unwrap()).unwrap();

        let installer = HookInstaller {
            settings_path: Some(tmp_file.clone()),
            server_port: Some(23334), // different port to verify rewrite
        };
        installer.register().expect("register should succeed");

        let contents = std::fs::read_to_string(&tmp_file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        let perm_arr = parsed["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_arr.len(), 1, "should have exactly one PermissionRequest entry");

        let entry = &perm_arr[0];
        // Must be nested format with matcher + hooks array
        assert!(entry.get("matcher").is_some(), "entry must have matcher field");
        let inner = entry["hooks"].as_array().expect("entry must have hooks array");
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0]["url"].as_str().unwrap(), "http://127.0.0.1:23334/permission");

        // Old flat url field should NOT exist at top level
        assert!(entry.get("url").is_none(), "flat url field must not exist at top level");
        assert!(entry.get("type").is_none(), "flat type field must not exist at top level");

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn test_permission_hook_healthy_nested() {
        let settings = serde_json::json!({
            "hooks": {
                "PermissionRequest": [{
                    "matcher": "",
                    "hooks": [{ "type": "http", "url": "http://127.0.0.1:23333/permission", "timeout": 600 }]
                }]
            }
        });
        assert!(permission_hook_is_healthy(&settings, "http://127.0.0.1:23333/permission"));
    }

    #[test]
    fn test_permission_hook_unhealthy_flat() {
        let settings = serde_json::json!({
            "hooks": {
                "PermissionRequest": [{
                    "type": "http", "url": "http://127.0.0.1:23333/permission", "timeout": 600
                }]
            }
        });
        assert!(!permission_hook_is_healthy(&settings, "http://127.0.0.1:23333/permission"));
    }

    #[test]
    fn test_permission_hook_unhealthy_wrong_port() {
        let settings = serde_json::json!({
            "hooks": {
                "PermissionRequest": [{
                    "matcher": "",
                    "hooks": [{ "type": "http", "url": "http://127.0.0.1:99999/permission", "timeout": 600 }]
                }]
            }
        });
        assert!(!permission_hook_is_healthy(&settings, "http://127.0.0.1:23333/permission"));
    }

    #[test]
    fn test_permission_hook_unhealthy_missing() {
        let settings = serde_json::json!({ "hooks": {} });
        assert!(!permission_hook_is_healthy(&settings, "http://127.0.0.1:23333/permission"));
    }
}
