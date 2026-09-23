//! Keyboard focus for the Browser module's tab views.
//!
//! Every tab is a GTK widget inside the app window (see `mod.rs`), so focus
//! is plain GTK focus: `grab_focus` on the WebKit widget puts keystrokes in
//! the page, and `grab_focus` on the app's own webview takes them back for
//! the address bar and shortcuts. No X11 involvement, and it works on
//! Wayland too.

use gtk::prelude::*;
use wry::{WebView, WebViewExtUnix};

pub fn focus_child(wv: &WebView) -> Result<(), String> {
    wv.webview().grab_focus();
    Ok(())
}

pub fn focus_app_window(window: &tauri::Window) -> Result<(), String> {
    let widget = super::host::app_webview(window)?;
    widget.grab_focus();
    Ok(())
}
