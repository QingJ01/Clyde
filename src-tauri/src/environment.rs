use tauri::{AppHandle, Manager};

use crate::prefs::SharedPrefs;
use crate::state_machine::SharedState;
use crate::util::MutexExt;

#[derive(Debug, Default, Clone, Copy)]
struct EnvironmentSnapshot {
    fullscreen_on_pet_monitor: bool,
    meeting_or_share_active: bool,
}

pub const CONTROLS_SUPPORTED: bool = cfg!(target_os = "macos");

pub fn controls_supported() -> bool {
    CONTROLS_SUPPORTED
}

pub fn start_environment_loop(app: &AppHandle, state: SharedState) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;

            let prefs = app
                .try_state::<SharedPrefs>()
                .map(|prefs| prefs.lock_or_recover().clone())
                .unwrap_or_default();

            if !prefs.auto_hide_fullscreen && !prefs.auto_dnd_meetings {
                crate::set_auto_hidden(&app, &state, false);
                crate::set_auto_dnd(&app, &state, false);
                continue;
            }

            let snapshot = detect_environment(&app);
            crate::set_auto_hidden(
                &app,
                &state,
                prefs.auto_hide_fullscreen && snapshot.fullscreen_on_pet_monitor,
            );
            crate::set_auto_dnd(
                &app,
                &state,
                prefs.auto_dnd_meetings && snapshot.meeting_or_share_active,
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn detect_environment(_app: &AppHandle) -> EnvironmentSnapshot {
    EnvironmentSnapshot::default()
}

#[cfg(target_os = "macos")]
fn detect_environment(app: &AppHandle) -> EnvironmentSnapshot {
    use crate::windows;
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use core_graphics::geometry::CGRect;
    use core_graphics::window::{
        create_description_from_array, create_window_list, kCGNullWindowID, kCGWindowBounds,
        kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        kCGWindowName, kCGWindowOwnerName,
    };

    let current_monitor = match windows::current_monitor_for_pet(app) {
        Some(monitor) => monitor,
        None => return EnvironmentSnapshot::default(),
    };
    let monitor_rect = monitor_rect(&current_monitor);
    let monitor_area = rect_area(&monitor_rect);

    let Some(window_ids) = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return EnvironmentSnapshot::default();
    };
    let Some(window_info) = create_description_from_array(window_ids) else {
        return EnvironmentSnapshot::default();
    };

    let mut snapshot = EnvironmentSnapshot::default();
    for dict in &window_info {
        let layer = get_i32(&dict, unsafe {
            CFString::wrap_under_get_rule(kCGWindowLayer)
        })
        .unwrap_or(0);
        if layer != 0 {
            continue;
        }

        let owner = get_string(&dict, unsafe {
            CFString::wrap_under_get_rule(kCGWindowOwnerName)
        });
        let title = get_string(&dict, unsafe {
            CFString::wrap_under_get_rule(kCGWindowName)
        });
        if should_ignore_window(&owner, &title) {
            continue;
        }

        let Some(bounds_dict) = dict
            .find(unsafe { CFString::wrap_under_get_rule(kCGWindowBounds) })
            .and_then(|value| value.downcast::<CFDictionary>())
        else {
            continue;
        };
        let Some(bounds) = CGRect::from_dict_representation(&bounds_dict) else {
            continue;
        };

        let overlap = intersection_area(&bounds, &monitor_rect);
        let overlap_ratio = if monitor_area > 0.0 {
            overlap / monitor_area
        } else {
            0.0
        };
        let window_ratio = if monitor_area > 0.0 {
            rect_area(&bounds) / monitor_area
        } else {
            0.0
        };

        if overlap_ratio >= 0.97 {
            snapshot.fullscreen_on_pet_monitor = true;
        }

        if is_meeting_window(&owner, &title) && (overlap_ratio >= 0.18 || window_ratio >= 0.18) {
            snapshot.meeting_or_share_active = true;
        }

        if snapshot.fullscreen_on_pet_monitor && snapshot.meeting_or_share_active {
            break;
        }
    }

    snapshot
}

#[cfg(target_os = "macos")]
fn get_string(
    dict: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
    key: core_foundation::string::CFString,
) -> String {
    dict.find(key)
        .and_then(|value| value.downcast::<core_foundation::string::CFString>())
        .map(|value| value.to_string())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn get_i32(
    dict: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
    key: core_foundation::string::CFString,
) -> Option<i32> {
    dict.find(key)
        .and_then(|value| value.downcast::<core_foundation::number::CFNumber>())
        .and_then(|value| value.to_i32())
}

#[cfg(target_os = "macos")]
fn should_ignore_window(owner: &str, title: &str) -> bool {
    let owner = owner.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    owner.is_empty()
        || owner == "clyde"
        || owner == "window server"
        || owner == "dock"
        || title.contains("clyde")
}

#[cfg(target_os = "macos")]
fn is_meeting_window(owner: &str, title: &str) -> bool {
    let owner = owner.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();

    let owner_keywords = [
        "zoom",
        "zoom.us",
        "microsoft teams",
        "teams",
        "webex",
        "slack",
        "feishu",
        "lark",
        "tencent meeting",
        "腾讯会议",
    ];
    let title_keywords = [
        "zoom meeting",
        "google meet",
        " meet ",
        "meeting",
        "huddle",
        "webex",
        "teams",
        "sharing",
        "presenting",
        "screen share",
        "share screen",
        "共享",
        "演示",
        "正在共享",
        "正在演示",
    ];

    owner_keywords.iter().any(|keyword| owner.contains(keyword))
        || title_keywords.iter().any(|keyword| title.contains(keyword))
}

#[cfg(target_os = "macos")]
fn monitor_rect(monitor: &crate::windows::MonitorArea) -> core_graphics::geometry::CGRect {
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};

    CGRect::new(
        &CGPoint::new(monitor.x as f64, monitor.y as f64),
        &CGSize::new(monitor.width as f64, monitor.height as f64),
    )
}

#[cfg(target_os = "macos")]
fn rect_area(rect: &core_graphics::geometry::CGRect) -> f64 {
    rect.size.width.max(0.0) * rect.size.height.max(0.0)
}

#[cfg(target_os = "macos")]
fn intersection_area(
    a: &core_graphics::geometry::CGRect,
    b: &core_graphics::geometry::CGRect,
) -> f64 {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let bottom = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    (right - left).max(0.0) * (bottom - top).max(0.0)
}
