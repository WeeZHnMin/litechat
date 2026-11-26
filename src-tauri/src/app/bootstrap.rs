use super::{paths, storage};
use tauri::{AppHandle, Manager};

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let _ = paths::ensure(app);
    // 默认工作目录在 .litechat/workspace/ 下，首次启动时建出来
    let workspace = paths::ensure_workspace(app)?;
    app.manage(storage::Storage::initialize(app)?);
    app.manage(crate::agent::models::ModelSettings::load(app));
    app.manage(crate::agent::chat::ActiveStreams::default());
    app.manage(crate::agent::chat::QueuedInput::default());
    app.manage(crate::agent::chat::QueuedImages::default());
    app.manage(crate::tools::mcp::McpManager::new(workspace));
    app.manage(crate::agent::permissions::Permissions::load(app));
    crate::tools::mcp::commands::initialize(app);
    Ok(())
}
