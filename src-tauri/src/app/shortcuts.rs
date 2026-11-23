use crate::app::window::{main_window, show_main_window, toggle_main_window};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutConfig {
    toggle: String,
    #[serde(default = "default_new_chat")]
    new_chat: String,
    #[serde(default = "default_history")]
    history: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutFailure {
    action: String,
    accelerator: String,
    reason: String,
}

pub struct ShortcutSettings {
    config: Mutex<ShortcutConfig>,
    failures: Mutex<Vec<ShortcutFailure>>,
    suspended: Mutex<bool>,
}

fn default_new_chat() -> String {
    "Alt+Shift+N".into()
}

fn default_history() -> String {
    "Alt+Shift+H".into()
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle: "Alt+Shift+L".into(),
            new_chat: default_new_chat(),
            history: default_history(),
        }
    }
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            config: Mutex::new(ShortcutConfig::default()),
            failures: Mutex::new(Vec::new()),
            suspended: Mutex::new(false),
        }
    }
}

fn shortcuts_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::app::paths::config_dir(app).map(|path| path.join("shortcuts.json"))
}

fn load_shortcuts(app: &AppHandle) -> ShortcutConfig {
    let Ok(path) = shortcuts_path(app) else {
        return ShortcutConfig::default();
    };
    let Ok(bytes) = fs::read(path) else {
        return ShortcutConfig::default();
    };
    let Ok(config) = serde_json::from_slice::<ShortcutConfig>(&bytes) else {
        return ShortcutConfig::default();
    };
    let valid = config.toggle.parse::<Shortcut>().is_ok()
        && config.new_chat.parse::<Shortcut>().is_ok()
        && config.history.parse::<Shortcut>().is_ok()
        && config.toggle != config.new_chat
        && config.toggle != config.history
        && config.new_chat != config.history;
    if valid {
        config
    } else {
        ShortcutConfig::default()
    }
}

fn save_shortcuts(app: &AppHandle, config: &ShortcutConfig) -> Result<(), String> {
    let path = shortcuts_path(app)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn entries(config: &ShortcutConfig) -> [(&'static str, &String); 3] {
    [
        ("toggle", &config.toggle),
        ("newChat", &config.new_chat),
        ("history", &config.history),
    ]
}

fn register_all(
    app: &AppHandle,
    config: &ShortcutConfig,
) -> (Vec<Shortcut>, Vec<ShortcutFailure>) {
    let shortcuts = app.global_shortcut();
    let mut registered = Vec::new();
    let mut failures = Vec::new();
    for (action, accelerator) in entries(config) {
        let parsed = match accelerator.parse::<Shortcut>() {
            Ok(shortcut) => shortcut,
            Err(error) => {
                failures.push(ShortcutFailure {
                    action: action.into(),
                    accelerator: accelerator.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        match shortcuts.register(parsed.clone()) {
            Ok(()) => registered.push(parsed),
            Err(error) => failures.push(ShortcutFailure {
                action: action.into(),
                accelerator: accelerator.clone(),
                reason: error.to_string(),
            }),
        }
    }
    (registered, failures)
}

pub fn register(app: &AppHandle) -> Result<(), String> {
    let config = load_shortcuts(app);
    let (_, failures) = register_all(app, &config);
    for failure in &failures {
        eprintln!(
            "LiteChat: 快捷键 {} ({}) 注册失败: {}",
            failure.action, failure.accelerator, failure.reason
        );
    }
    let state = app.state::<ShortcutSettings>();
    *state.config.lock().map_err(|error| error.to_string())? = config;
    *state.failures.lock().map_err(|error| error.to_string())? = failures;
    Ok(())
}

pub fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let settings = app.state::<ShortcutSettings>();
    if settings.suspended.lock().map(|value| *value).unwrap_or(false) {
        return;
    }
    let Ok(config) = settings.config.lock() else {
        return;
    };
    let action = if config.toggle.parse::<Shortcut>().ok().as_ref() == Some(shortcut) {
        Some("toggle")
    } else if config.new_chat.parse::<Shortcut>().ok().as_ref() == Some(shortcut) {
        Some("newChat")
    } else if config.history.parse::<Shortcut>().ok().as_ref() == Some(shortcut) {
        Some("history")
    } else {
        None
    };
    drop(config);
    let Some(window) = main_window(app) else {
        return;
    };
    match action {
        Some("toggle") => toggle_main_window(&window),
        Some("newChat") => {
            show_main_window(&window);
            let _ = window.emit("shortcut:new-chat", ());
        }
        Some("history") => {
            show_main_window(&window);
            let _ = window.emit("shortcut:history", ());
        }
        _ => {}
    }
}

#[tauri::command]
pub fn get_shortcuts(state: tauri::State<ShortcutSettings>) -> Result<ShortcutConfig, String> {
    state
        .config
        .lock()
        .map(|config| config.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_shortcut_failures(
    state: tauri::State<ShortcutSettings>,
) -> Result<Vec<ShortcutFailure>, String> {
    state
        .failures
        .lock()
        .map(|failures| failures.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_shortcuts_suspended(
    app: AppHandle,
    suspended: bool,
    state: tauri::State<ShortcutSettings>,
) -> Result<(), String> {
    *state.suspended.lock().map_err(|error| error.to_string())? = suspended;
    let shortcuts = app.global_shortcut();
    let config = state.config.lock().map_err(|error| error.to_string())?.clone();
    for (_, accelerator) in entries(&config) {
        let Ok(shortcut) = accelerator.parse::<Shortcut>() else {
            continue;
        };
        if suspended {
            let _ = shortcuts.unregister(shortcut);
        } else {
            let _ = shortcuts.register(shortcut);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn update_shortcut(
    app: AppHandle,
    toggle: String,
    new_chat: String,
    history: String,
) -> Result<ShortcutConfig, String> {
    if toggle == new_chat || toggle == history || new_chat == history {
        return Err("快捷键不能重复".into());
    }
    let next = ShortcutConfig {
        toggle,
        new_chat,
        history,
    };
    let next_shortcuts = [
        next.toggle.parse::<Shortcut>().map_err(|error| error.to_string())?,
        next.new_chat.parse::<Shortcut>().map_err(|error| error.to_string())?,
        next.history.parse::<Shortcut>().map_err(|error| error.to_string())?,
    ];
    let state = app.state::<ShortcutSettings>();
    let mut config = state.config.lock().map_err(|error| error.to_string())?;
    let previous_shortcuts = [
        config.toggle.parse::<Shortcut>().map_err(|error| error.to_string())?,
        config.new_chat.parse::<Shortcut>().map_err(|error| error.to_string())?,
        config.history.parse::<Shortcut>().map_err(|error| error.to_string())?,
    ];
    let shortcuts = app.global_shortcut();
    for shortcut in &previous_shortcuts {
        shortcuts.unregister(shortcut.clone()).map_err(|error| error.to_string())?;
    }
    let rollback = || {
        for shortcut in &next_shortcuts {
            let _ = shortcuts.unregister(shortcut.clone());
        }
        for shortcut in &previous_shortcuts {
            let _ = shortcuts.register(shortcut.clone());
        }
    };
    for shortcut in &next_shortcuts {
        if let Err(error) = shortcuts.register(shortcut.clone()) {
            rollback();
            return Err(error.to_string());
        }
    }
    if let Err(error) = save_shortcuts(&app, &next) {
        rollback();
        return Err(error);
    }
    *config = next.clone();
    if let Ok(mut failures) = state.failures.lock() {
        failures.clear();
    }
    Ok(next)
}
