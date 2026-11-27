#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod app;
mod tools;


fn main() {
    tauri::Builder::default()
        .manage(app::shortcuts::ShortcutSettings::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    app::shortcuts::handle_shortcut(app, shortcut, event.state);
                })
                .build(),
        )
        .setup(|app| {
            app::bootstrap::initialize(app.handle())?;
            app::shortcuts::register(app.handle())?;
            app::tray::setup(app)?;
            if let Some(window) = app::window::main_window(app.handle()) {
                app::window::install_close_to_tray(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::window::minimize_window,
            app::window::toggle_always_on_top,
            app::shortcuts::get_shortcuts,
            app::shortcuts::set_shortcuts_suspended,
            app::shortcuts::update_shortcut,
            app::storage::create_conversation,
            app::storage::list_conversations,
            app::storage::append_message,
            app::storage::load_messages,
            agent::models::get_model_config,
            agent::models::save_model_config,
            agent::models::clear_model_api_key,
            agent::models::model_config_path,
            agent::models::test_model_config,
            agent::models::list_models,
            agent::models::set_reasoning_effort,
            agent::models::set_active_model,
            agent::models::list_saved_models,
            agent::chat::send_chat,
            agent::chat::cancel_stream,
            agent::chat::queue_user_input,
            tools::mcp::commands::get_mcp_servers,
            tools::mcp::commands::save_mcp_servers,
            tools::mcp::commands::refresh_mcp,
            tools::mcp::commands::get_workspace,
            tools::mcp::commands::set_workspace,
            tools::mcp::commands::save_attachment,
            tools::mcp::commands::save_folder,
            tools::mcp::commands::import_folder,
            tools::mcp::commands::open_workspace_file,
            agent::permissions::respond_approval,
            agent::permissions::get_permission_state,
            agent::permissions::set_approval_mode,
            app::clipboard::clipboard_has_image,
            app::clipboard::read_clipboard_image,
            app::clipboard::capture_with_snipping_tool,
            app::clipboard::copy_image_to_clipboard
        ])
        .run(tauri::generate_context!())
        .expect("error while running LiteChat");
}
