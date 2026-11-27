use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 用户主目录下的应用目录，存放配置、数据库和默认工作目录。
const APP_DIR: &str = "LiteChat";

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
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
    if let Some(home) = home_dir() {
        return Ok(home.join(APP_DIR));
    }
    app.path()
        .app_config_dir()
        .map_err(|error| error.to_string())
}

const WORKSPACE_FILE: &str = "workspace.txt";

/// 不直接用 `.litechat/` 本身，避免模型误碰配置和数据库。
const DEFAULT_WORKSPACE_DIR: &str = "workspace";

fn workspace_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(WORKSPACE_FILE))
}

pub fn default_workspace() -> PathBuf {
    home_dir()
        .map(|home| home.join(APP_DIR).join(DEFAULT_WORKSPACE_DIR))
        .unwrap_or_else(std::env::temp_dir)
}

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

/// 旧版本把配置放在项目根或 Tauri 默认的 appConfig 目录，
/// 首次启动时把这些文件搬过来，避免用户丢失历史记录。
fn migrate_legacy(app: &AppHandle, target: &Path) {
    let mut sources: Vec<PathBuf> = Vec::new();
    // 开发时项目根目录下的 .litechat
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe
            .ancestors()
            .skip(1)
            .find(|dir| dir.join("src-tauri").join("tauri.conf.json").is_file())
        {
            sources.push(root.join(".litechat"));
        }
    }
    if let Ok(legacy) = app.path().app_config_dir() {
        sources.push(legacy);
    }

    for source in sources {
        if source == target || !source.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&source) else {
            continue;
        };
        let mut items: Vec<(String, PathBuf)> = entries
            .flatten()
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().to_string(),
                    entry.path(),
                )
            })
            .collect();

        // SQLite 的 -wal 和 -shm 必须跟主库一起迁移，且主库要先落位；
        // read_dir 顺序不保证，这里显式排序。
        items.sort_by_key(|(name, _)| match name.as_str() {
            "history.db" => 0,
            "history.db-wal" => 1,
            "history.db-shm" => 2,
            _ => 3,
        });

        // 目标已有主库时说明迁移过一次了，别再拷半套 SQLite 文件
        let db_exists = target.join("history.db").exists();

        for (name, path) in items {
            if db_exists && name.starts_with("history.db") {
                continue;
            }
            let destination = target.join(path.file_name().unwrap_or_default());
            if path.is_file() {
                if !destination.exists() {
                    let _ = fs::copy(&path, &destination);
                }
            } else if path.is_dir() {
                // 目录已存在也要往里补，否则空目录会挡掉整个子树的迁移
                let _ = copy_dir(&path, &destination);
            }
        }
    }
}

fn copy_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)?.flatten() {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else if !target.exists() {
            let _ = fs::copy(&path, &target);
        }
    }
    Ok(())
}

pub fn ensure(app: &AppHandle) -> Result<PathBuf, String> {
    let target = config_dir(app)?;
    fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    migrate_legacy(app, &target);
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_is_a_subdirectory_of_the_app_dir() {
        assert_eq!(DEFAULT_WORKSPACE_DIR, "workspace");
        let expected = home_dir()
            .map(|home| home.join(APP_DIR).join(DEFAULT_WORKSPACE_DIR))
            .unwrap();
        assert!(expected.ends_with("workspace"));
    }

    #[test]
    fn default_workspace_points_into_the_user_home() {
        let workspace = default_workspace();
        assert!(
            workspace.ends_with(Path::new(APP_DIR).join(DEFAULT_WORKSPACE_DIR)),
            "默认工作目录应在 用户目录/{APP_DIR}/{DEFAULT_WORKSPACE_DIR} 下：{}",
            workspace.display()
        );
    }

    #[test]
    fn copy_dir_merges_into_existing_directory() {
        let base = std::env::temp_dir().join(format!("litechat-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let source = base.join("source");
        let target = base.join("target");
        fs::create_dir_all(source.join("uploads")).unwrap();
        fs::write(source.join("uploads/a.txt"), b"file").unwrap();
        // 目标里已有同名目录，但没有内容
        fs::create_dir_all(&target).unwrap();

        copy_dir(&source, &target).unwrap();

        assert!(
            target.join("uploads/a.txt").is_file(),
            "已存在的目录也要补齐内容"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn app_dir_name_is_not_hidden() {
        // 放在用户主目录下，不该是点开头的隐藏目录
        assert!(!APP_DIR.starts_with('.'));
    }
}
