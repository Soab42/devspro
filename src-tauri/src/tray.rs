//! System tray icon + autostart. Kept out of `modules/` since it isn't a feature page,
//! just app-shell plumbing that `lib.rs` wires up alongside the window.

use crate::db::Db;
use crate::modules::settings;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("show", "Show DevsPro")
        .separator()
        .text("quit", "Quit")
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("app bundle icon missing; tray icon needs one");

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn close_to_tray_enabled(app: &AppHandle) -> bool {
    let db = app.state::<Db>();
    let Ok(conn) = db.0.lock() else {
        return false;
    };
    settings::get(&conn, "close_to_tray").as_deref() == Some("true")
}

#[tauri::command]
pub fn autostart_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn autostart_is_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}
