use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const CONFIG_DIR: &str = ".litechat";
const CONFIG_FILES: [&str; 2] = ["models.json", "shortcuts.json"];

fn project_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .find(|dir| dir.join("src-tauri").join("tauri.conf.json").is_file())
        .map(Path::to_path_buf)
}

fn overridden_dir() -> Option<PathBuf> {
    let value = std::env::var("LITECHAT_CONFIG_DIR").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = overridden_dir() {
        return Ok(dir);
    }
    if let Some(root) = project_root() {
        return Ok(root.join(CONFIG_DIR));
    }
    app.path()
        .app_config_dir()
        .map_err(|error| error.to_string())
}

const WORKSPACE_FILE: &str = "workspace.txt";

fn workspace_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(WORKSPACE_FILE))
}

pub fn default_workspace() -> PathBuf {
    project_root().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."))
}

pub fn workspace(app: &AppHandle) -> PathBuf {
    workspace_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| path.is_dir())
        .unwrap_or_else(default_workspace)
}

pub fn set_workspace(app: &AppHandle, value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("工作目录不能为空".into());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_dir() {
        return Err(format!("目录不存在: {trimmed}"));
    }
    let absolute = path.canonicalize().map_err(|error| error.to_string())?;
    let target = workspace_path(app)?;
    if let Some(directory) = target.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    fs::write(target, absolute.display().to_string()).map_err(|error| error.to_string())?;
    Ok(absolute)
}

pub fn ensure(app: &AppHandle) -> Result<PathBuf, String> {
    let target = config_dir(app)?;
    fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    if let Ok(legacy) = app.path().app_config_dir() {
        if legacy != target {
            for name in CONFIG_FILES {
                let source = legacy.join(name);
                let destination = target.join(name);
                if source.is_file() && !destination.exists() {
                    let _ = fs::copy(&source, &destination);
                }
            }
        }
    }
    Ok(target)
}
