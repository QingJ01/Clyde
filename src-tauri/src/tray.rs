use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager,
};
use crate::i18n::t;
use crate::util::MutexExt;
use crate::mini;
use crate::prefs::{self, SharedPrefs};
use crate::state_machine::SharedState;
use crate::windows;

pub type SharedTray = Arc<Mutex<Option<TrayIcon>>>;

fn build_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let quit   = MenuItem::with_id(app, "quit",   t("quit", lang),   true, None::<&str>)?;
    let dnd    = MenuItem::with_id(app, "dnd",    t("dnd", lang),    true, None::<&str>)?;
    let size_s = MenuItem::with_id(app, "size-s", "S",               true, None::<&str>)?;
    let size_m = MenuItem::with_id(app, "size-m", "M",               true, None::<&str>)?;
    let size_l = MenuItem::with_id(app, "size-l", "L",               true, None::<&str>)?;
    let size_sub = Submenu::with_items(app, t("size", lang), true, &[&size_s, &size_m, &size_l])?;

    let en_label = if lang == "en" { "✓ English" } else { "English" };
    let zh_label = if lang == "zh" { "✓ 中文" } else { "中文" };
    let lang_en = MenuItem::with_id(app, "lang-en", en_label, true, None::<&str>)?;
    let lang_zh = MenuItem::with_id(app, "lang-zh", zh_label, true, None::<&str>)?;
    let lang_sub = Submenu::with_items(app, t("language", lang), true, &[&lang_en, &lang_zh])?;

    let mini = MenuItem::with_id(app, "mini", t("mini", lang), true, None::<&str>)?;
    let autostart = MenuItem::with_id(app, "autostart", t("autoStart", lang), true, None::<&str>)?;

    Menu::with_items(app, &[&dnd, &mini, &size_sub, &lang_sub, &autostart, &quit])
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
                Ok(menu) => { let _ = tray.set_menu(Some(menu)); }
                Err(e) => eprintln!("Clyde: rebuild menu failed: {e}"),
            }
        }
    }
}

/// Shared helper: apply a new window size from any context.
pub fn apply_size_pub(app: &AppHandle, size_str: &str) { apply_size(app, size_str); }
fn apply_size(app: &AppHandle, size_str: &str) {
    if let Some(pet) = app.get_webview_window("pet") {
        let (w, h) = prefs::size_to_pixels(size_str);
        let _ = pet.set_size(tauri::PhysicalSize::new(w, h));
        if let Some(bounds) = windows::get_pet_bounds(app) {
            windows::sync_hit_window(app, &bounds, &windows::HitBox::DEFAULT);
        }
    }
    if let Some(prefs_state) = app.try_state::<SharedPrefs>() {
        let mut p = prefs_state.lock_or_recover();
        p.size = size_str.to_string();
        prefs::save(app, &p);
    }
}

/// Shared helper: apply a new language from any context.
pub fn apply_lang_pub(app: &AppHandle, lang: &str) { apply_lang(app, lang); }
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
        "dnd"  => {
            if let Some(state) = app.try_state::<SharedState>() {
                crate::do_toggle_dnd(app, &state);
            }
        },
        "autostart" => {
            if let Some(prefs_state) = app.try_state::<SharedPrefs>() {
                let mut p = prefs_state.lock_or_recover();
                p.auto_start_with_claude = !p.auto_start_with_claude;
                prefs::save(app, &p);
            }
        },
        "mini" => {
            if prefs::is_mini_mode(app) {
                mini::do_exit_mini(app);
            } else {
                mini::do_enter_mini(app);
            }
        },
        "size-s" | "size-m" | "size-l" => {
            let size_str = match id { "size-m" => "M", "size-l" => "L", _ => "S" };
            apply_size(app, size_str);
        },
        "lang-en" | "lang-zh" => {
            let lang = if id == "lang-zh" { "zh" } else { "en" };
            apply_lang(app, lang);
        },
        _ => {}
    }
}
