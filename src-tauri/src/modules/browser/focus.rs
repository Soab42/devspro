//! Keyboard focus for the Browser module's tab views.
//!
//! Every tab is a GTK widget inside the app window (see `mod.rs`), so focus
//! is plain GTK focus: `grab_focus` on the WebKit widget puts keystrokes in
//! the page, and `grab_focus` on the app's own webview takes them back for
//! the address bar and shortcuts. No X11 involvement, and it works on
//! Wayland too.

//!
//! Off Linux each tab is a native child window (WebView2 on Windows), and focus
//! is the OS's window focus: wry's own `focus` / `focus_parent`.

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use wry::WebView;
#[cfg(target_os = "linux")]
use wry::WebViewExtUnix;

#[cfg(target_os = "linux")]
pub fn focus_child(wv: &WebView) -> Result<(), String> {
    wv.webview().grab_focus();
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn focus_app_window(window: &tauri::Window) -> Result<(), String> {
    let widget = super::host::app_webview(window)?;
    widget.grab_focus();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn focus_child(wv: &WebView) -> Result<(), String> {
    wv.focus().map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn focus_app_window(window: &tauri::Window) -> Result<(), String> {
    window.set_focus().map_err(|e| e.to_string())
}
