use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_TASKS: usize = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub text: String,
    pub order: usize,
}

fn tasks_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".clyde").join("tasks.json"))
}

pub fn load_tasks() -> Vec<Task> {
    let Some(path) = tasks_path() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_tasks(tasks: &[Task]) {
    let Some(path) = tasks_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(tasks) {
        let _ = std::fs::write(path, json);
    }
}

#[tauri::command]
pub fn close_tasks_editor(app: AppHandle) {
    if let Some(win) = app.get_webview_window("tasks") {
        let _ = win.destroy();
    }
}

/// Emit task list to the pet window frontend.
pub fn emit_tasks(app: &AppHandle) {
    let tasks = get_tasks();
    let _ = tauri::Emitter::emit(app, "tasks-changed", tasks);
}

#[tauri::command]
pub fn get_tasks() -> Vec<Task> {
    let mut tasks = load_tasks();
    tasks.sort_by_key(|t| t.order);
    tasks
}

#[tauri::command]
pub fn set_tasks(app: AppHandle, tasks: Vec<Task>) {
    let mut tasks = tasks;
    tasks.truncate(MAX_TASKS);
    for (i, t) in tasks.iter_mut().enumerate() {
        t.order = i;
        if t.id.is_empty() {
            t.id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        }
    }
    save_tasks(&tasks);
    emit_tasks(&app);
}

#[tauri::command]
pub fn update_task(app: AppHandle, id: String, text: String) {
    let mut tasks = load_tasks();
    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
        task.text = text;
        save_tasks(&tasks);
        emit_tasks(&app);
    }
}

#[tauri::command]
pub fn add_task(app: AppHandle, text: String) {
    let mut tasks = load_tasks();
    if tasks.len() >= MAX_TASKS {
        return;
    }
    let order = tasks.len();
    tasks.push(Task {
        id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
        text,
        order,
    });
    save_tasks(&tasks);
    emit_tasks(&app);
}

#[tauri::command]
pub fn remove_task(app: AppHandle, id: String) {
    let mut tasks = load_tasks();
    tasks.retain(|t| t.id != id);
    for (i, t) in tasks.iter_mut().enumerate() {
        t.order = i;
    }
    save_tasks(&tasks);
    emit_tasks(&app);
}

#[tauri::command]
pub fn reorder_tasks(app: AppHandle, ids: Vec<String>) {
    let tasks = load_tasks();
    let mut reordered = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if let Some(mut t) = tasks.iter().find(|t| &t.id == id).cloned() {
            t.order = i;
            reordered.push(t);
        }
    }
    for t in &tasks {
        if !ids.contains(&t.id) {
            let mut t = t.clone();
            t.order = reordered.len();
            reordered.push(t);
        }
    }
    save_tasks(&reordered);
    emit_tasks(&app);
}
