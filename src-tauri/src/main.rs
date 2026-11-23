#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod chat;
mod mcp;
mod models;
mod paths;
mod permissions;
mod providers;
mod shortcuts;
mod storage;
mod tray;
mod window;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(shortcuts::ShortcutSettings::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    shortcuts::handle_shortcut(app, shortcut, event.state);
                })
                .build(),
        )
        .setup(|app| {
            let _ = paths::ensure(app.handle());
            app.manage(storage::Storage::initialize(app.handle())?);
            app.manage(models::ModelSettings::load(app.handle()));
            app.manage(chat::ActiveStreams::default());
            app.manage(mcp::McpManager::new(paths::workspace(app.handle())));
            app.manage(permissions::Permissions::default());
            mcp::commands::initialize(app.handle());
            shortcuts::register(app.handle())?;
            tray::setup(app)?;
            if let Some(window) = window::main_window(app.handle()) {
                window::install_close_to_tray(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window::minimize_window,
            window::toggle_always_on_top,
            shortcuts::get_shortcuts,
            shortcuts::get_shortcut_failures,
            shortcuts::set_shortcuts_suspended,
            shortcuts::update_shortcut,
            storage::create_conversation,
            storage::list_conversations,
            storage::append_message,
            storage::update_message,
            storage::load_messages,
            storage::delete_conversation,
            models::get_model_config,
            models::save_model_config,
            models::clear_model_api_key,
            models::model_config_path,
            chat::send_chat,
            chat::cancel_stream,
            mcp::commands::get_mcp_servers,
            mcp::commands::get_mcp_tools,
            mcp::commands::save_mcp_servers,
            mcp::commands::refresh_mcp,
            mcp::commands::get_workspace,
            mcp::commands::set_workspace,
            permissions::respond_approval,
            permissions::get_permission_state,
            permissions::set_approval_mode,
            permissions::reset_tool_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running LiteChat");
}
