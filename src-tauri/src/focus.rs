#[cfg(not(target_os = "windows"))]
use std::process::Command;

#[cfg(target_os = "windows")]
pub fn focus_window_by_pid(pid: u32, _cwd: &str) {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    // Run in a dedicated OS thread to ensure proper stack and Win32 state.
    // EnumWindows callbacks are extern "system" — panics there cause non-unwinding abort.
    let _ = std::thread::Builder::new()
        .name("focus-window".into())
        .spawn(move || {
            struct SearchData {
                target_pid: u32,
                found_hwnd: isize,
            }
            let mut search = SearchData {
                target_pid: pid,
                found_hwnd: 0,
            };

            unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let search = &mut *(lparam.0 as *mut SearchData);
                let mut window_pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
                if window_pid == search.target_pid && IsWindowVisible(hwnd).as_bool() {
                    search.found_hwnd = hwnd.0 as isize;
                    return BOOL(0); // stop enumeration
                }
                BOOL(1) // continue
            }

            unsafe {
                let lparam = LPARAM(&mut search as *mut SearchData as isize);
                let _ = EnumWindows(Some(enum_callback), lparam);
                if search.found_hwnd != 0 {
                    let hwnd = HWND(search.found_hwnd as *mut std::ffi::c_void);
                    // Bypass Windows foreground lock with ALT key event
                    keybd_event(0x12, 0, KEYBD_EVENT_FLAGS(0), 0);
                    keybd_event(0x12, 0, KEYEVENTF_KEYUP, 0);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        });
}

#[cfg(target_os = "macos")]
pub fn focus_window_by_pid(pid: u32, _cwd: &str) {
    // Non-blocking: spawn a dedicated thread like the Windows implementation,
    // so this can be safely called from async HTTP handlers.
    let _ = std::thread::Builder::new()
        .name("focus-window".into())
        .spawn(move || {
            let script = format!(
                r#"tell application "System Events"
                    set theProcess to first process whose unix id is {}
                    set frontmost of theProcess to true
                end tell"#,
                pid
            );
            let _ = Command::new("osascript").arg("-e").arg(&script).status();
        });
}

#[cfg(target_os = "linux")]
pub fn focus_window_by_pid(pid: u32, _cwd: &str) {
    // Non-blocking: spawn a dedicated thread like the Windows implementation,
    // so this can be safely called from async HTTP handlers.
    let _ = std::thread::Builder::new()
        .name("focus-window".into())
        .spawn(move || {
            let result = Command::new("wmctrl")
                .args(["-ip", &pid.to_string()])
                .status();
            if result.map(|s| !s.success()).unwrap_or(true) {
                let _ = Command::new("xdotool")
                    .args(["search", "--pid", &pid.to_string(), "windowfocus"])
                    .status();
            }
        });
}

#[tauri::command]
pub fn focus_terminal_for_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state_machine::SharedState>,
    bubbles: tauri::State<'_, crate::permission::BubbleMap>,
    session_id: Option<String>,
    pid: Option<u32>,
    cwd: Option<String>,
) {
    use crate::util::MutexExt;
    let fallback = session_id.as_deref().and_then(|session_id| {
        let sm = state.lock_or_recover();
        sm.sessions
            .get(session_id)
            .map(|entry| (entry.source_pid, entry.cwd.clone()))
    });

    let target_pid = pid.or_else(|| fallback.as_ref().and_then(|(pid, _)| *pid));
    let target_cwd = cwd
        .or_else(|| fallback.as_ref().map(|(_, cwd)| cwd.clone()))
        .unwrap_or_default();

    if let Some(p) = target_pid {
        focus_window_by_pid(p, &target_cwd);
    }
    crate::dismiss_transient_ui(&app, &state, &bubbles);
}
