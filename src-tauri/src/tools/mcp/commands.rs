use crate::tools::mcp::{McpManager, McpServerConfig, McpServerStatus};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::app::paths::config_dir(app).map(|path| path.join("mcp.json"))
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
                .filter(|config| config.name != crate::tools::mcp::BUILTIN_SERVER)
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
    Ok(app.state::<McpManager>().workspace().display().to_string())
}

/// 用系统默认程序打开工作目录内的文件。
#[tauri::command]
pub fn open_workspace_file(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let root = app.state::<McpManager>().workspace();
    let target = crate::tools::mcp::files::resolve(&root, &path)?;
    if !target.is_file() {
        return Err(format!("文件不存在：{path}"));
    }
    app.opener()
        .open_path(target.to_string_lossy(), None::<&str>)
        .map_err(|error| format!("无法打开文件：{error}"))
}

/// 保存上传的附件到工作目录的 uploads/ 下，返回相对路径。
/// 消息里只带路径和摘要，模型需要内容时用 read_document 去读。
#[tauri::command]
pub fn save_attachment(app: AppHandle, name: String, data: String) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|error| format!("附件数据无效：{error}"))?;
    let root = app.state::<McpManager>().workspace();
    let path = crate::tools::mcp::uploads::save(&root, &name, &bytes)?;
    // 只保留最近 50 个附件，避免目录无限膨胀
    crate::tools::mcp::uploads::prune(&root, 50);
    Ok(path)
}

#[tauri::command]
pub fn set_workspace(app: AppHandle, path: String) -> Result<String, String> {
    let resolved = crate::app::paths::set_workspace(&app, &path)?;
    app.state::<McpManager>().set_workspace_path(resolved.clone());
    Ok(resolved.display().to_string())
}

#[tauri::command]
pub async fn get_mcp_servers(app: AppHandle) -> Result<Vec<McpServerStatus>, String> {
    Ok(app.state::<McpManager>().statuses().await)
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

pub async fn install_server(
    app: &AppHandle,
    name: &str,
    command: &str,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
) -> Result<String, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("服务名称不能为空".into());
    }
    if trimmed_name == crate::tools::mcp::BUILTIN_SERVER {
        return Err("shell 是保留名称，请换一个".into());
    }
    if command.trim().is_empty() {
        return Err("启动命令不能为空".into());
    }
    let mut configs = load(app);
    if configs.iter().any(|item| item.name == trimmed_name) {
        return Err(format!("已存在名为 {trimmed_name} 的服务，请直接使用或先移除"));
    }
    let previous = configs.clone();
    configs.push(McpServerConfig {
        name: trimmed_name.to_string(),
        command: command.trim().to_string(),
        args,
        env,
        enabled: true,
    });

    let manager = app.state::<McpManager>();
    manager.set_configs(configs.clone());
    let tools = manager.refresh().await;
    let status = manager.statuses().await;
    let entry = status
        .iter()
        .find(|item| item.name == trimmed_name)
        .ok_or("保存后未找到该服务")?;

    if let Some(error) = &entry.error {
        manager.set_configs(previous.clone());
        let _ = save(app, &previous);
        manager.refresh().await;
        return Err(format!("连接失败，已回滚：{error}"));
    }
    save(app, &configs)?;
    let names: Vec<String> = tools
        .iter()
        .filter(|tool| tool.server == trimmed_name)
        .map(|tool| tool.name.clone())
        .collect();
    if names.is_empty() {
        return Ok(format!(
            "已添加服务 {trimmed_name}，但它没有声明任何工具"
        ));
    }
    Ok(format!(
        "已添加服务 {trimmed_name}，可用工具 {} 个：{}",
        names.len(),
        names.join("、")
    ))
}

pub async fn uninstall_server(app: &AppHandle, name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed == crate::tools::mcp::BUILTIN_SERVER {
        return Err("shell 是内置服务，不能移除".into());
    }
    let mut configs = load(app);
    let before = configs.len();
    configs.retain(|item| item.name != trimmed);
    if configs.len() == before {
        return Err(format!("没有找到名为 {trimmed} 的服务"));
    }
    let manager = app.state::<McpManager>();
    manager.set_configs(configs.clone());
    manager.refresh().await;
    save(app, &configs)?;
    Ok(format!("已移除服务 {trimmed}，它的工具不再可用"))
}
