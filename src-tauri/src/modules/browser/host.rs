//! Where a tab's WebKit widget lives inside the app window.
//!
//! wry's `build_as_child` was the first host: a raw X11 window parked over
//! the content area. It painted fine but was never a proper citizen of the
//! GTK toplevel — X input focus never moved into it (no typing), the first
//! click into a page left GDK with a stale implicit grab (no hover
//! afterwards), and none of it existed on Wayland.
//!
//! So instead the tab is an ordinary GTK widget in the app's own window
//! tree. Tauri lays its window out as `GtkApplicationWindow > GtkBox >
//! WebKitWebView`. On first use per window that becomes `GtkApplicationWindow
//! > GtkOverlay > WebKitWebView`, with a `gtk::Fixed` laid over the webview
//! that holds the tabs. The overlay child is marked pass-through, so where the
//! Fixed has no tab the app's own webview still gets the pointer; where it
//! does, the tab is a real widget with real GTK focus, hover, and cursor
//! handling. The box is dropped rather than wrapped because Tauri's
//! undecorated-resize hook requires the webview's grandparent to be the
//! window.
//!
//! The Fixed is remembered per window label; a popped-out window gets its own.

use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static HOSTS: RefCell<HashMap<String, gtk::Fixed>> = RefCell::new(HashMap::new());
}

/// Tauri's own webview widget in `window`: the vbox child that is a WebKit view.
pub fn app_webview(window: &tauri::Window) -> Result<gtk::Widget, String> {
    let vbox = window.default_vbox().map_err(|e| e.to_string())?;
    find_webview(vbox.upcast_ref())
        .ok_or_else(|| "could not find the app's webview in its window".to_string())
}

fn find_webview(container: &gtk::Container) -> Option<gtk::Widget> {
    for child in container.children() {
        if child.type_().name() == "WebKitWebView" {
            return Some(child);
        }
        if let Some(inner) = child.downcast_ref::<gtk::Container>() {
            if let Some(found) = find_webview(inner) {
                return Some(found);
            }
        }
    }
    None
}

/// The `gtk::Fixed` that tabs in `window` are put into, built on first use.
pub fn container_for(window: &tauri::Window) -> Result<gtk::Fixed, String> {
    let label = window.label().to_string();
    if let Some(fixed) = HOSTS.with(|h| h.borrow().get(&label).cloned()) {
        return Ok(fixed);
    }

    let vbox = window.default_vbox().map_err(|e| e.to_string())?;
    let gtk_window = window.gtk_window().map_err(|e| e.to_string())?;
    let webview = app_webview(window)?;
    if vbox.children().len() > 1 {
        return Err("the window has a menu bar; the in-app browser can't overlay it".into());
    }
    // Tauri's undecorated-resize hook reads `webview.parent().parent()` and
    // insists it is the GtkWindow, so the overlay must sit exactly where the
    // vbox was — the vbox itself goes (Tauri keeps its handle; nothing else
    // is ever packed into it here).
    vbox.remove(&webview);
    gtk_window.remove(&vbox);

    let overlay = gtk::Overlay::new();
    overlay.add(&webview);
    let fixed = gtk::Fixed::new();
    overlay.add_overlay(&fixed);
    overlay.set_overlay_pass_through(&fixed, true);
    gtk_window.add(&overlay);
    overlay.show_all();

    HOSTS.with(|h| h.borrow_mut().insert(label, fixed.clone()));
    Ok(fixed)
}

/// Place a tab's widget: `gtk::Fixed` positions by `move_` and sizes by the
/// widget's size request, and both survive relayouts, unlike a one-off
/// `size_allocate`.
pub fn place(fixed: &gtk::Fixed, widget: &gtk::Widget, x: f64, y: f64, width: f64, height: f64) {
    fixed.move_(widget, x.round() as i32, y.round() as i32);
    widget.set_size_request(width.max(1.0).round() as i32, height.max(1.0).round() as i32);
}

/// Take a tab's widget out of its Fixed before the view is dropped.
pub fn remove(widget: &gtk::Widget) {
    if let Some(parent) = widget.parent() {
        if let Some(container) = parent.downcast_ref::<gtk::Container>() {
            container.remove(widget);
        }
    }
}
