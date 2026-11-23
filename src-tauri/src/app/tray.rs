use crate::app::window::{main_window, show_main_window, toggle_main_window};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::App;

pub fn setup(app: &App) -> Result<(), tauri::Error> {
    let show = MenuItem::with_id(app, "show", "显示 LiteChat", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏到托盘", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;
    TrayIconBuilder::new()
        .icon(app.default_window_icon().expect("window icon").clone())
        .tooltip("LiteChat")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = main_window(app) {
                    show_main_window(&window);
                }
            }
            "hide" => {
                if let Some(window) = main_window(app) {
                    let _ = window.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Some(window) = main_window(tray.app_handle()) {
                    toggle_main_window(&window);
                }
            }
        })
        .build(app)?;
    Ok(())
}
