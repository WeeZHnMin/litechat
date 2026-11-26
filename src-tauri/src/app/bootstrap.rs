use super::{paths, storage};
use tauri::{AppHandle, Manager};

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let _ = paths::ensure(app);
    app.manage(storage::Storage::initialize(app)?);
    app.manage(crate::agent::models::ModelSettings::load(app));
    app.manage(crate::agent::chat::ActiveStreams::default());
    app.manage(crate::agent::chat::QueuedInput::default());
    app.manage(crate::tools::mcp::McpManager::new(paths::workspace(app)));
    app.manage(crate::agent::permissions::Permissions::load(app));
    crate::tools::mcp::commands::initialize(app);
    Ok(())
}
