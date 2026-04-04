use crate::prefs::{self, SharedPrefs};
use crate::util::MutexExt;
use serde::Deserialize;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/QingJ01/Clyde/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 4 * 60 * 60; // 4 hours
const STARTUP_DELAY_SECS: u64 = 30;
const REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub notes: String,
}

/// Compare two semver strings (e.g. "0.2.0" > "0.1.5").
/// Strips leading 'v' if present.
fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let r = parse(remote);
    let c = parse(current);
    for i in 0..r.len().max(c.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if rv > cv {
            return true;
        }
        if rv < cv {
            return false;
        }
    }
    false
}

async fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .user_agent("clyde-on-desk")
        .build()
        .map_err(|e| format!("http client error: {e}"))?;

    let resp = client
        .get(GITHUB_RELEASES_URL)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    let release: GitHubRelease = resp
        .json()
        .await
        .map_err(|e| format!("json parse error: {e}"))?;

    let version = release.tag_name.trim_start_matches('v').to_string();
    Ok(ReleaseInfo {
        version,
        url: release.html_url,
        notes: release.body.unwrap_or_default(),
    })
}

fn epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn start_update_check_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Don't slow down startup
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        loop {
            check_once(&app).await;
            tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        }
    });
}

/// Trigger an immediate manual update check (ignores interval and dismissed version).
pub fn trigger_manual_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        do_check(&app, true).await;
    });
}

async fn check_once(app: &AppHandle) {
    do_check(app, false).await;
}

async fn do_check(app: &AppHandle, manual: bool) {
    let Some(prefs_state) = app.try_state::<SharedPrefs>() else {
        return;
    };

    let (last_check, dismissed) = {
        let p = prefs_state.lock_or_recover();
        (
            p.last_update_check_epoch,
            p.dismissed_update_version.clone(),
        )
    };

    // Auto-check respects minimum interval; manual check always proceeds
    if !manual {
        let now = epoch_now();
        if now.saturating_sub(last_check) < CHECK_INTERVAL_SECS {
            return;
        }
    }

    let release = match fetch_latest_release().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Clyde: update check failed: {e}");
            if manual {
                show_status_bubble(app, "checkFailed", &e);
            }
            return;
        }
    };

    // Persist check timestamp
    {
        let mut p = prefs_state.lock_or_recover();
        p.last_update_check_epoch = epoch_now();
        prefs::save(app, &p);
    }

    let current = env!("CARGO_PKG_VERSION");
    if !is_newer(&release.version, current) {
        if manual {
            let lang = get_lang(app);
            let desc = format!(
                "{} (v{current})",
                crate::i18n::t("upToDateDesc", &lang),
            );
            show_status_bubble(app, "upToDate", &desc);
        }
        return;
    }

    // Auto-check skips dismissed versions; manual check shows them again
    if !manual && release.version == dismissed {
        return;
    }

    println!(
        "Clyde: update available — current={current} latest={}",
        release.version
    );
    show_update_bubble(app, &release);
}

fn get_lang(app: &AppHandle) -> String {
    app.try_state::<SharedPrefs>()
        .map(|p| p.lock_or_recover().lang.clone())
        .unwrap_or_else(|| "en".into())
}

/// Show a transient ModeNotice-style bubble for status messages (up-to-date, error).
fn show_status_bubble(app: &AppHandle, title_key: &str, description: &str) {
    let Some(bubbles) = app.try_state::<crate::permission::BubbleMap>() else {
        return;
    };

    let lang = get_lang(app);
    let title = crate::i18n::t(title_key, &lang);

    let data = crate::permission::BubbleData {
        id: uuid::Uuid::new_v4().to_string(),
        window_kind: crate::permission::WindowKind::ModeNotice,
        tool_name: String::new(),
        tool_input: serde_json::Value::Null,
        suggestions: Vec::new(),
        session_id: String::new(),
        agent_label: String::new(),
        session_summary: String::new(),
        session_project: String::new(),
        session_short_id: String::new(),
        is_elicitation: false,
        elicitation_message: None,
        elicitation_schema: None,
        elicitation_mode: None,
        elicitation_url: None,
        elicitation_server_name: None,
        mode_label: Some(format!("✅ {title}")),
        mode_description: Some(description.to_string()),
        update_version: None,
        update_url: None,
        update_notes: None,
        update_lang: None,
    };

    let id = data.id.clone();
    if crate::permission::show_bubble(app, &bubbles, data) {
        // Auto-dismiss after 4 seconds
        let app2 = app.clone();
        let bubbles2 = (*bubbles).clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(4)).await;
            crate::permission::close_bubble(&app2, &bubbles2, &id);
        });
    }
}

fn show_update_bubble(app: &AppHandle, release: &ReleaseInfo) {
    let Some(bubbles) = app.try_state::<crate::permission::BubbleMap>() else {
        return;
    };

    // Truncate release notes for display
    let notes = if release.notes.len() > 300 {
        format!("{}…", &release.notes[..300])
    } else {
        release.notes.clone()
    };

    let lang = get_lang(app);

    let data = crate::permission::BubbleData {
        id: "update-check".into(),
        window_kind: crate::permission::WindowKind::UpdateNotice,
        tool_name: String::new(),
        tool_input: serde_json::Value::Null,
        suggestions: Vec::new(),
        session_id: String::new(),
        agent_label: String::new(),
        session_summary: String::new(),
        session_project: String::new(),
        session_short_id: String::new(),
        is_elicitation: false,
        elicitation_message: None,
        elicitation_schema: None,
        elicitation_mode: None,
        elicitation_url: None,
        elicitation_server_name: None,
        mode_label: None,
        mode_description: None,
        update_version: Some(release.version.clone()),
        update_url: Some(release.url.clone()),
        update_notes: Some(notes),
        update_lang: Some(lang),
    };

    crate::permission::show_bubble(app, &bubbles, data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_basic() {
        assert!(is_newer("v0.2.0", "0.1.5"));
        assert!(is_newer("0.2.0", "0.1.5"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.1.6", "0.1.5"));
    }

    #[test]
    fn test_is_newer_equal() {
        assert!(!is_newer("0.1.5", "0.1.5"));
        assert!(!is_newer("v0.1.5", "0.1.5"));
    }

    #[test]
    fn test_is_newer_older() {
        assert!(!is_newer("0.1.4", "0.1.5"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
    }

    #[test]
    fn test_is_newer_different_lengths() {
        assert!(is_newer("0.2", "0.1.5"));
        assert!(!is_newer("0.1", "0.1.5"));
    }
}
