use crate::i18n::t;
use crate::mini;
use crate::prefs::{self, SharedPrefs};
use crate::state_machine::SharedState;
use crate::util::MutexExt;
use crate::windows;
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager,
};

pub type SharedTray = Arc<Mutex<Option<TrayIcon>>>;

fn restore_interaction_label(lang: &str, click_through: bool, locked: bool) -> String {
    let base = t("restoreInteraction", lang);
    if lang == "zh" {
        match (click_through, locked) {
            (true, true) => format!("{base}（关闭穿透/锁定）"),
            (true, false) => format!("{base}（关闭穿透）"),
            (false, true) => format!("{base}（关闭锁定）"),
            (false, false) => base,
        }
    } else {
        match (click_through, locked) {
            (true, true) => format!("{base} (disable click through / lock)"),
            (true, false) => format!("{base} (disable click through)"),
            (false, true) => format!("{base} (disable lock)"),
            (false, false) => base,
        }
    }
}

fn build_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let prefs = app
        .try_state::<SharedPrefs>()
        .map(|state| state.lock_or_recover().clone());
    let size = prefs
        .as_ref()
        .map(|prefs| prefs.size.as_str())
        .unwrap_or("S");
    let opacity = prefs
        .as_ref()
        .map(|prefs| (prefs.opacity * 100.0).round() as i32)
        .unwrap_or(100);
    let is_locked = prefs
        .as_ref()
        .map(|prefs| prefs.lock_position)
        .unwrap_or(false);
    let click_through = prefs
        .as_ref()
        .map(|prefs| prefs.click_through)
        .unwrap_or(false);
    let auto_hide_fullscreen = prefs
        .as_ref()
        .map(|prefs| prefs.auto_hide_fullscreen)
        .unwrap_or(false);
    let auto_dnd_meetings = prefs
        .as_ref()
        .map(|prefs| prefs.auto_dnd_meetings)
        .unwrap_or(false);
    let permission_decision_window_secs = prefs
        .as_ref()
        .map(|prefs| prefs.permission_decision_window_secs)
        .unwrap_or(crate::prefs::DEFAULT_PERMISSION_DECISION_WINDOW_SECS);
    let environment_controls_supported = crate::environment::controls_supported();
    let autostart_enabled = prefs
        .as_ref()
        .map(|prefs| prefs.auto_start_with_claude)
        .unwrap_or(false);
    let is_mini = prefs.as_ref().map(|prefs| prefs.mini_mode).unwrap_or(false);
    let is_dnd = app
        .try_state::<SharedState>()
        .map(|state| state.lock_or_recover().dnd)
        .unwrap_or(false);

    let quit = MenuItem::with_id(app, "quit", t("quit", lang), true, None::<&str>)?;
    let dnd_label = if is_dnd {
        format!("✓ {}", t("dnd", lang))
    } else {
        t("dnd", lang)
    };
    let dnd = MenuItem::with_id(app, "dnd", dnd_label, true, None::<&str>)?;
    let size_s = MenuItem::with_id(
        app,
        "size-s",
        if size == "S" { "✓ S" } else { "S" },
        true,
        None::<&str>,
    )?;
    let size_m = MenuItem::with_id(
        app,
        "size-m",
        if size == "M" { "✓ M" } else { "M" },
        true,
        None::<&str>,
    )?;
    let size_l = MenuItem::with_id(
        app,
        "size-l",
        if size == "L" { "✓ L" } else { "L" },
        true,
        None::<&str>,
    )?;
    let size_sub = Submenu::with_items(app, t("size", lang), true, &[&size_s, &size_m, &size_l])?;

    let mut opacity_items = Vec::new();
    for level in [100, 90, 80, 70, 60, 50, 40] {
        let label = if opacity == level {
            format!("✓ {level}%")
        } else {
            format!("{level}%")
        };
        opacity_items.push(MenuItem::with_id(
            app,
            format!("opacity-{level}"),
            label,
            true,
            None::<&str>,
        )?);
    }
    let opacity_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = opacity_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let opacity_sub = Submenu::with_items(app, t("opacity", lang), true, &opacity_refs)?;

    let mut permission_wait_items = Vec::new();
    for secs in [12_u16, 20, 30, 45, 60] {
        let label = if permission_decision_window_secs == secs {
            format!("✓ {secs}s")
        } else {
            format!("{secs}s")
        };
        permission_wait_items.push(MenuItem::with_id(
            app,
            format!("permission-timeout-{secs}"),
            label,
            true,
            None::<&str>,
        )?);
    }
    let permission_wait_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = permission_wait_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let permission_wait_sub = Submenu::with_items(
        app,
        t("permissionWaitTime", lang),
        true,
        &permission_wait_refs,
    )?;

    let en_label = if lang == "en" {
        "✓ English"
    } else {
        "English"
    };
    let zh_label = if lang == "zh" { "✓ 中文" } else { "中文" };
    let lang_en = MenuItem::with_id(app, "lang-en", en_label, true, None::<&str>)?;
    let lang_zh = MenuItem::with_id(app, "lang-zh", zh_label, true, None::<&str>)?;
    let lang_sub = Submenu::with_items(app, t("language", lang), true, &[&lang_en, &lang_zh])?;

    let mini_label = if is_mini {
        format!("✓ {}", t("mini", lang))
    } else {
        t("mini", lang)
    };
    let mini = MenuItem::with_id(app, "mini", mini_label, true, None::<&str>)?;
    let restore_interaction = MenuItem::with_id(
        app,
        "restore-interaction",
        restore_interaction_label(lang, click_through, is_locked),
        true,
        None::<&str>,
    )?;
    let lock_label = if is_locked {
        format!("✓ {}", t("lockPosition", lang))
    } else {
        t("lockPosition", lang)
    };
    let lock_position = MenuItem::with_id(app, "lock-position", lock_label, true, None::<&str>)?;
    let click_label = if click_through {
        format!("✓ {}", t("clickThrough", lang))
    } else {
        t("clickThrough", lang)
    };
    let click_through_item =
        MenuItem::with_id(app, "click-through", click_label, true, None::<&str>)?;
    let fullscreen_label = crate::platform_limited_menu_label(
        "hideOnFullscreen",
        lang,
        auto_hide_fullscreen,
        environment_controls_supported,
    );
    let fullscreen_hide = MenuItem::with_id(
        app,
        "hide-on-fullscreen",
        fullscreen_label,
        environment_controls_supported,
        None::<&str>,
    )?;
    let auto_dnd_label = crate::platform_limited_menu_label(
        "autoDndMeetings",
        lang,
        auto_dnd_meetings,
        environment_controls_supported,
    );
    let auto_dnd = MenuItem::with_id(
        app,
        "auto-dnd-meetings",
        auto_dnd_label,
        environment_controls_supported,
        None::<&str>,
    )?;
    let autostart_label = if autostart_enabled {
        format!("✓ {}", t("autoStart", lang))
    } else {
        t("autoStart", lang)
    };
    let autostart = MenuItem::with_id(app, "autostart", autostart_label, true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &dnd,
            &mini,
            &restore_interaction,
            &lock_position,
            &click_through_item,
            &fullscreen_hide,
            &auto_dnd,
            &size_sub,
            &opacity_sub,
            &permission_wait_sub,
            &lang_sub,
            &autostart,
            &quit,
        ],
    )
}

pub fn build_tray(app: &AppHandle, lang: &str) -> tauri::Result<TrayIcon> {
    let menu = build_menu(app, lang)?;

    TrayIconBuilder::new()
        .icon(match app.default_window_icon() {
            Some(icon) => icon.clone(),
            None => return Err(tauri::Error::AssetNotFound("window icon".to_string())),
        })
        .menu(&menu)
        .on_menu_event(|app, event| handle_tray_event(app, event.id().as_ref()))
        .build(app)
}

pub fn rebuild_menu(app: &AppHandle, lang: &str) {
    if let Some(tray_state) = app.try_state::<SharedTray>() {
        let guard = tray_state.lock_or_recover();
        if let Some(tray) = guard.as_ref() {
            match build_menu(app, lang) {
                Ok(menu) => {
                    let _ = tray.set_menu(Some(menu));
                }
                Err(e) => eprintln!("Clyde: rebuild menu failed: {e}"),
            }
        }
    }
}

fn rebuild_current_menu(app: &AppHandle) {
    if let Some(lang) = app
        .try_state::<SharedPrefs>()
        .map(|prefs| prefs.lock_or_recover().lang.clone())
    {
        rebuild_menu(app, &lang);
    }
}

/// Shared helper: apply a new window size from any context.
pub fn apply_size_pub(app: &AppHandle, size_str: &str) {
    apply_size(app, size_str);
}
fn apply_size(app: &AppHandle, size_str: &str) {
    if let Some(pet) = app.get_webview_window("pet") {
        let (w, h) = prefs::size_to_pixels(size_str);
        let _ = pet.set_size(tauri::PhysicalSize::new(w, h));
        if let Some(bounds) = windows::get_pet_bounds(app) {
            windows::sync_hit_window(app, &bounds, &windows::HitBox::INTERACTIVE);
        }
    }
    if let Some(prefs_state) = app.try_state::<SharedPrefs>() {
        let mut p = prefs_state.lock_or_recover();
        p.size = size_str.to_string();
        prefs::save(app, &p);
    }
}

/// Shared helper: apply a new language from any context.
pub fn apply_lang_pub(app: &AppHandle, lang: &str) {
    apply_lang(app, lang);
}
fn apply_lang(app: &AppHandle, lang: &str) {
    if let Some(prefs_state) = app.try_state::<SharedPrefs>() {
        let mut p = prefs_state.lock_or_recover();
        p.lang = lang.to_string();
        prefs::save(app, &p);
    }
    let _ = app.emit("lang-changed", lang);
    rebuild_menu(app, lang);
}

fn handle_tray_event(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "dnd" => {
            if let Some(state) = app.try_state::<SharedState>() {
                crate::do_toggle_dnd(app, &state);
            }
            rebuild_current_menu(app);
        }
        "autostart" => {
            crate::toggle_autostart_pref(app);
            rebuild_current_menu(app);
        }
        "mini" => {
            if prefs::is_mini_mode(app) {
                mini::do_exit_mini(app);
            } else {
                mini::do_enter_mini(app);
            }
            rebuild_current_menu(app);
        }
        "restore-interaction" => {
            crate::restore_interaction(app);
            rebuild_current_menu(app);
        }
        "lock-position" => {
            crate::toggle_position_lock_pref(app);
            rebuild_current_menu(app);
        }
        "click-through" => {
            crate::toggle_click_through_pref(app);
            rebuild_current_menu(app);
        }
        "hide-on-fullscreen" => {
            crate::toggle_auto_hide_fullscreen_pref(app);
            rebuild_current_menu(app);
        }
        "auto-dnd-meetings" => {
            crate::toggle_auto_dnd_meetings_pref(app);
            rebuild_current_menu(app);
        }
        "size-s" | "size-m" | "size-l" => {
            let size_str = match id {
                "size-m" => "M",
                "size-l" => "L",
                _ => "S",
            };
            apply_size(app, size_str);
            rebuild_current_menu(app);
        }
        "opacity-100" | "opacity-90" | "opacity-80" | "opacity-70" | "opacity-60"
        | "opacity-50" | "opacity-40" => {
            let pct = id
                .strip_prefix("opacity-")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(100);
            crate::set_opacity(app, pct as f32 / 100.0);
            rebuild_current_menu(app);
        }
        "permission-timeout-12"
        | "permission-timeout-20"
        | "permission-timeout-30"
        | "permission-timeout-45"
        | "permission-timeout-60" => {
            let secs = id
                .strip_prefix("permission-timeout-")
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(crate::prefs::DEFAULT_PERMISSION_DECISION_WINDOW_SECS);
            crate::set_permission_decision_window_secs(app, secs);
            rebuild_current_menu(app);
        }
        "lang-en" | "lang-zh" => {
            let lang = if id == "lang-zh" { "zh" } else { "en" };
            apply_lang(app, lang);
        }
        _ => {}
    }
}
