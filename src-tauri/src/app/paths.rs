use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

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

/// 不直接用应用目录本身，避免模型误碰配置和数据库。
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
        // 旧版本存进去的路径可能带 `\\?\` 前缀，读出来先剥掉
        .map(|text| strip_verbatim(Path::new(text.trim())))
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

const MAX_WORKSPACE_HISTORY: usize = 10;

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("workspaces.json"))
}

pub fn workspace_history(app: &AppHandle) -> Vec<String> {
    history_path(app)
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|path| Path::new(path).is_dir())
        .collect()
}

/// 删除历史记录；delete_files 为 true 时连同磁盘目录一起删。
pub fn remove_workspace(
    app: &AppHandle,
    path: &str,
    delete_files: bool,
) -> Result<Vec<String>, String> {
    let target_dir = PathBuf::from(path);
    let current = workspace(app);
    if current == target_dir
        || strip_verbatim(&current) == strip_verbatim(&target_dir)
    {
        return Err("不能删除正在使用的工作目录，请先切换到其他目录".into());
    }

    if is_dangerous_path(&target_dir) {
        return Err(format!("出于安全考虑，不能删除 {path}"));
    }

    let target = history_path(app)?;
    let mut list = workspace_history(app);
    let before = list.len();
    list.retain(|item| item != path);
    let in_history = list.len() != before;

    if delete_files {
        if !target_dir.exists() {
            return Err(format!("目录不存在：{path}"));
        }
        fs::remove_dir_all(&target_dir)
            .map_err(|error| format!("删除目录失败：{error}"))?;
    } else if !in_history {
        return Err(format!("历史里没有这个目录：{path}"));
    }

    if let Some(directory) = target.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&list).map_err(|error| error.to_string())?;
    fs::write(target, bytes).map_err(|error| error.to_string())?;
    Ok(list)
}

fn is_dangerous_path(path: &Path) -> bool {
    let normalized = strip_verbatim(path);
    if !normalized.is_absolute() {
        return true;
    }
    let text = normalized.display().to_string();
    let trimmed = text.trim_end_matches(['\\', '/']);
    if trimmed.len() <= 2 && trimmed.ends_with(':') {
        return true;
    }

    if let Some(home) = home_dir() {
        if strip_verbatim(&home) == normalized {
            return true;
        }
    }

    // 用户主目录的子目录要放行，否则整棵 C:\Users 都会被挡掉
    let home = home_dir().map(|home| strip_verbatim(&home));
    let inside_home = home.as_ref().is_some_and(|home| is_within(&normalized, home));
    if !inside_home && protected_roots().iter().any(|root| is_within(&normalized, root)) {
        return true;
    }

    // 桌面、文档等用户目录同样不可恢复；AppData 整棵子树都保护
    if let Some(home) = home {
        let user_folders = ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos"];
        for name in user_folders {
            if normalized == home.join(name) {
                return true;
            }
        }
        if is_within(&normalized, &home.join("AppData")) {
            return true;
        }
    }
    false
}

fn protected_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Windows"),
        PathBuf::from(r"C:\Program Files"),
        PathBuf::from(r"C:\Program Files (x86)"),
        PathBuf::from(r"C:\ProgramData"),
        PathBuf::from(r"C:\Users"),
        PathBuf::from(r"C:\$Recycle.Bin"),
        PathBuf::from(r"C:\System Volume Information"),
        PathBuf::from(r"C:\Recovery"),
        PathBuf::from(r"C:\Boot"),
        PathBuf::from(r"C:\PerfLogs"),
    ];
    for drive in ['D', 'E', 'F', 'G'] {
        roots.push(PathBuf::from(format!(r"{drive}:\System Volume Information")));
        roots.push(PathBuf::from(format!(r"{drive}:\$Recycle.Bin")));
    }
    roots
}

fn is_within(path: &Path, root: &Path) -> bool {
    if path == root {
        return true;
    }
    path.starts_with(root)
}

fn remember_workspace(app: &AppHandle, path: &Path) {
    let value = path.display().to_string();
    let Ok(target) = history_path(app) else {
        return;
    };
    let mut list = workspace_history(app);
    list.retain(|item| item != &value);
    list.insert(0, value);
    list.truncate(MAX_WORKSPACE_HISTORY);
    if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
        let _ = fs::write(target, bytes);
    }
}

/// 去掉 Windows canonicalize 产生的 `\\?\` 前缀。
fn strip_verbatim(path: &Path) -> PathBuf {
    const VERBATIM_UNC: &str = r"\\?\UNC\";
    const VERBATIM: &str = r"\\?\";
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(VERBATIM_UNC) {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(VERBATIM) {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
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
    let absolute = path
        .canonicalize()
        .map(|resolved| strip_verbatim(&resolved))
        .map_err(|error| error.to_string())?;
    let target = workspace_path(app)?;
    if let Some(directory) = target.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    fs::write(target, absolute.display().to_string()).map_err(|error| error.to_string())?;
    remember_workspace(app, &absolute);
    Ok(absolute)
}

/// 从旧位置迁移配置，避免升级后丢失历史。
fn migrate_legacy(app: &AppHandle, target: &Path) {
    let mut sources: Vec<PathBuf> = Vec::new();
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

        // SQLite 的 -wal/-shm 必须跟主库一起迁移，顺序有依赖
        items.sort_by_key(|(name, _)| match name.as_str() {
            "history.db" => 0,
            "history.db-wal" => 1,
            "history.db-shm" => 2,
            _ => 3,
        });

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
        fs::create_dir_all(&target).unwrap();

        copy_dir(&source, &target).unwrap();

        assert!(
            target.join("uploads/a.txt").is_file(),
            "已存在的目录也要补齐内容"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn history_keeps_most_recent_first_and_dedups() {
        let mut list: Vec<String> = vec!["/a".into(), "/b".into()];
        let value = "/c".to_string();
        list.retain(|item| item != &value);
        list.insert(0, value.clone());
        assert_eq!(list, vec!["/c", "/a", "/b"]);

        list.retain(|item| item != "/a");
        list.insert(0, "/a".into());
        assert_eq!(list, vec!["/a", "/c", "/b"]);
    }

    #[test]
    fn history_caps_at_limit() {
        let mut list: Vec<String> = (0..MAX_WORKSPACE_HISTORY).map(|i| format!("/p{i}")).collect();
        list.insert(0, "/new".into());
        list.truncate(MAX_WORKSPACE_HISTORY);
        assert_eq!(list.len(), MAX_WORKSPACE_HISTORY);
        assert_eq!(list[0], "/new");
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        let plain = Path::new(r"C:\Users\me\workspace");
        assert_eq!(strip_verbatim(plain), plain);

        let verbatim = Path::new(r"\\?\C:\Users\me\workspace");
        assert_eq!(strip_verbatim(verbatim), plain);

        let unc = Path::new(r"\\?\UNC\server\share");
        assert_eq!(strip_verbatim(unc), PathBuf::from(r"\\server\share"));
    }

    #[test]
    fn dangerous_paths_are_rejected() {
        assert!(is_dangerous_path(Path::new("C:\\")));
        assert!(is_dangerous_path(Path::new("D:")));
        // 系统目录的子目录同样危险
        assert!(is_dangerous_path(Path::new("C:\\Windows\\System32")));
        assert!(is_dangerous_path(Path::new("C:\\Program Files\\App")));
        assert!(is_dangerous_path(Path::new("C:\\Users")));
        assert!(is_dangerous_path(Path::new("C:\\$Recycle.Bin")));
        if let Some(home) = home_dir() {
            let home = strip_verbatim(&home);
            assert!(is_dangerous_path(&home), "主目录本身不能删");
            assert!(is_dangerous_path(&home.join("Desktop")));
            assert!(is_dangerous_path(&home.join("AppData").join("Local")));
            assert!(
                !is_dangerous_path(&home.join("projects").join("demo")),
                "主目录下的普通子目录应放行"
            );
        }
        // 其他盘的普通目录放行
        assert!(!is_dangerous_path(Path::new("D:\\Projects\\demo")));
    }

    #[test]
    fn forget_removes_only_the_target() {
        let mut list: Vec<String> = vec!["/a".into(), "/b".into(), "/c".into()];
        let target = "/b".to_string();
        let before = list.len();
        list.retain(|item| item != &target);
        assert_eq!(list.len(), before - 1);
        assert_eq!(list, vec!["/a", "/c"]);
    }

    #[test]
    fn forgetting_missing_entry_is_rejected() {
        let list: Vec<String> = vec!["/a".into()];
        let target = "/missing".to_string();
        let before = list.len();
        let after: Vec<String> = list.into_iter().filter(|item| item != &target).collect();
        assert_eq!(before, after.len(), "不存在的条目不应改变列表");
    }

    #[test]
    fn app_dir_name_is_not_hidden() {
        assert!(!APP_DIR.starts_with('.'));
    }
}
