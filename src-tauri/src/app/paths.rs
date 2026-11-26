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

/// 文件工具的默认工作目录，放在应用数据目录下单独的子目录里。
/// 不直接用 `.litechat/` 本身，避免模型误碰配置和数据库。
const DEFAULT_WORKSPACE_DIR: &str = "workspace";

fn workspace_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(WORKSPACE_FILE))
}

pub fn default_workspace() -> PathBuf {
    // 优先用用户主目录，拿不到再退回临时目录
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// 工作目录：用户显式设置过就用那个，否则落到应用数据目录下的 workspace/。
pub fn workspace(app: &AppHandle) -> PathBuf {
    workspace_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| {
            config_dir(app)
                .map(|dir| dir.join(DEFAULT_WORKSPACE_DIR))
                .unwrap_or_else(|_| default_workspace())
        })
}

/// 确保默认工作目录存在，供启动时调用。
pub fn ensure_workspace(app: &AppHandle) -> Result<PathBuf, String> {
    let path = workspace(app);
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workspace_dir_is_not_the_config_dir_itself() {
        // 工作目录和配置/数据库要分开，避免模型误碰 .litechat 里的文件
        assert_ne!(DEFAULT_WORKSPACE_DIR, "");
        assert!(DEFAULT_WORKSPACE_DIR.contains("workspace"));
    }

    #[test]
    fn workspace_lives_under_config_dir() {
        // 没有 workspace.txt 时，路径 = <config_dir>/workspace
        let config = PathBuf::from(r"C:\data\.litechat");
        let expected = config.join(DEFAULT_WORKSPACE_DIR);
        assert!(expected.ends_with("workspace"));
        assert!(expected.starts_with(r"C:\data\.litechat"));
    }

    #[test]
    fn fallback_workspace_is_a_directory() {
        let fallback = default_workspace();
        assert!(fallback.is_dir(), "回退目录应存在：{}", fallback.display());
    }
}
