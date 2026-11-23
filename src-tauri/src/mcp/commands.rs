use crate::mcp::{McpManager, McpServerConfig, McpServerStatus, McpToolInfo};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::paths::config_dir(app).map(|path| path.join("mcp.json"))
}

fn save(app: &AppHandle, configs: &[McpServerConfig]) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(configs).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn load(app: &AppHandle) -> Vec<McpServerConfig> {
    config_path(app)
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<McpServerConfig>>(&bytes).ok())
        .map(|configs| {
            configs
                .into_iter()
                .filter(|config| config.name != crate::mcp::BUILTIN_SERVER)
                .collect()
        })
        .unwrap_or_default()
}

pub fn initialize(app: &AppHandle) {
    let manager = app.state::<McpManager>();
    manager.set_configs(load(app));
}

#[tauri::command]
pub fn get_workspace(app: AppHandle) -> Result<String, String> {
    Ok(crate::paths::workspace(&app).display().to_string())
}

#[tauri::command]
pub fn set_workspace(app: AppHandle, path: String) -> Result<String, String> {
    let resolved = crate::paths::set_workspace(&app, &path)?;
    Ok(resolved.display().to_string())
}

#[tauri::command]
pub async fn get_mcp_servers(app: AppHandle) -> Result<Vec<McpServerStatus>, String> {
    Ok(app.state::<McpManager>().statuses().await)
}

#[tauri::command]
pub async fn get_mcp_tools(app: AppHandle) -> Result<Vec<McpToolInfo>, String> {
    Ok(app.state::<McpManager>().tools().await)
}

#[tauri::command]
pub async fn save_mcp_servers(
    app: AppHandle,
    servers: Vec<McpServerConfig>,
) -> Result<Vec<McpServerStatus>, String> {
    for server in &servers {
        if server.name.trim().is_empty() {
            return Err("服务名称不能为空".into());
        }
        if server.command.trim().is_empty() {
            return Err(format!("{} 缺少启动命令", server.name));
        }
    }
    let mut names = std::collections::HashSet::new();
    for server in &servers {
        if !names.insert(server.name.trim().to_string()) {
            return Err(format!("服务名称重复：{}", server.name));
        }
    }
    save(&app, &servers)?;
    let manager = app.state::<McpManager>();
    manager.set_configs(servers);
    manager.refresh().await;
    Ok(manager.statuses().await)
}

#[tauri::command]
pub async fn refresh_mcp(app: AppHandle) -> Result<Vec<McpServerStatus>, String> {
    let manager = app.state::<McpManager>();
    manager.refresh().await;
    Ok(manager.statuses().await)
}
