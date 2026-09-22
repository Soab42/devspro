//! Browser — real web pages inside the app window.
//!
//! An `<iframe>` was the obvious first idea and it doesn't work: most sites
//! (and every site that sends `X-Frame-Options`) refuse to be framed, and the
//! ones that allow it still can't be inspected, zoomed or navigated by us.
//!
//! So each tab is a native child webview, created with Tauri's `unstable`
//! multi-webview support and parked at the rectangle the page's content area
//! occupies. React draws the tabs, address bar and skin; the native view draws
//! the page. The two never overlap by design: the frontend reports the content
//! rectangle with a ResizeObserver and we move the child to match.
//!
//! Consequences worth knowing:
//!   - A native child always paints above the app's DOM, so the page hides
//!     every tab that isn't active, and hides all of them when the user leaves
//!     the Browser route. Otherwise a web page would sit on top of the Docker
//!     table.
//!   - We can't ask the page "can you go back?" — that would need IPC into a
//!     foreign origin. Back and forward are `history.back()`/`forward()` via
//!     eval, and the buttons stay enabled; a no-op click costs nothing.
//!   - Titles and URLs come from wry's own hooks, not the page, so a hostile
//!     page can't spoof the address bar.
//!
//! The webview label is `browser-<id>` with a client-chosen id, for the same
//! reason as PTY sessions: the frontend registers its event listeners before
//! the view exists, so the first title change is never missed.

use serde::{Deserialize, Serialize};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};
use tauri::Url;

pub const TITLE_EVENT: &str = "browser:title";
pub const LOAD_EVENT: &str = "browser:load";
pub const NEW_WINDOW_EVENT: &str = "browser:new-window";

#[derive(Debug, Clone, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
struct TitleEvent {
    id: String,
    title: String,
}

#[derive(Debug, Clone, Serialize)]
struct LoadEvent {
    id: String,
    url: String,
    /// "started" or "finished".
    state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct NewWindowEvent {
    id: String,
    url: String,
}

fn label(id: &str) -> String {
    format!("browser-{id}")
}

/// Only http(s) leaves the app: `file://`, `tauri://` and friends would let a
/// tab read the app's own assets or the disk.
fn checked_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|e| format!("invalid URL: {e}"))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(format!("{other}: URLs cannot be opened here")),
    }
}

fn webview(app: &AppHandle, id: &str) -> Result<tauri::Webview, String> {
    app.get_webview(&label(id))
        .ok_or_else(|| "that browser tab is no longer open".to_string())
}

#[tauri::command]
pub fn browser_open(app: AppHandle, id: String, url: String, bounds: Bounds) -> Result<(), String> {
    let url = checked_url(&url)?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window is gone".to_string())?;
    if app.get_webview(&label(&id)).is_some() {
        return Err("that tab already exists".into());
    }

    let (title_id, load_id, new_id) = (id.clone(), id.clone(), id.clone());
    let load_app = app.clone();
    let new_app = app.clone();
    let builder = WebviewBuilder::new(label(&id), WebviewUrl::External(url))
        .on_document_title_changed(move |wv, title| {
            let _ = wv.app_handle().emit(
                TITLE_EVENT,
                TitleEvent {
                    id: title_id.clone(),
                    title,
                },
            );
        })
        .on_page_load(move |_wv, payload| {
            let state = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };
            let _ = load_app.emit(
                LOAD_EVENT,
                LoadEvent {
                    id: load_id.clone(),
                    url: payload.url().to_string(),
                    state,
                },
            );
        })
        // target=_blank and window.open become a new tab in our strip instead
        // of a bare OS window with no chrome at all.
        .on_new_window(move |url, _features| {
            let _ = new_app.emit(
                NEW_WINDOW_EVENT,
                NewWindowEvent {
                    id: new_id.clone(),
                    url: url.to_string(),
                },
            );
            NewWindowResponse::Deny
        });

    window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width, bounds.height),
        )
        .map_err(|e| format!("could not create the browser view: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let url = checked_url(&url)?;
    webview(&app, &id)?
        .navigate(url)
        .map_err(|e| format!("could not navigate: {e}"))
}

#[tauri::command]
pub fn browser_back(app: AppHandle, id: String) -> Result<(), String> {
    webview(&app, &id)?
        .eval("history.back()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_forward(app: AppHandle, id: String) -> Result<(), String> {
    webview(&app, &id)?
        .eval("history.forward()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_reload(app: AppHandle, id: String) -> Result<(), String> {
    webview(&app, &id)?.reload().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_stop(app: AppHandle, id: String) -> Result<(), String> {
    webview(&app, &id)?
        .eval("window.stop()")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_set_bounds(app: AppHandle, id: String, bounds: Bounds) -> Result<(), String> {
    let wv = webview(&app, &id)?;
    wv.set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(|e| e.to_string())?;
    wv.set_size(LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_set_visible(app: AppHandle, id: String, visible: bool) -> Result<(), String> {
    let wv = webview(&app, &id)?;
    if visible { wv.show() } else { wv.hide() }.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_set_zoom(app: AppHandle, id: String, factor: f64) -> Result<(), String> {
    webview(&app, &id)?
        .set_zoom(factor.clamp(0.25, 5.0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_close(app: AppHandle, id: String) -> Result<(), String> {
    // Closing a tab that already went away is fine: the strip and the
    // native side can disagree briefly during teardown.
    match app.get_webview(&label(&id)) {
        Some(wv) => wv.close().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// Every open tab, so a remounted page (the user came back to the route) can
/// re-adopt views that survived while it was away.
#[tauri::command]
pub fn browser_list(app: AppHandle) -> Vec<String> {
    app.webviews()
        .keys()
        .filter_map(|l| l.strip_prefix("browser-").map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_schemes_are_accepted() {
        assert!(checked_url("https://example.com/a?b=1").is_ok());
        assert!(checked_url("http://localhost:8080").is_ok());
        assert!(checked_url("file:///etc/passwd").unwrap_err().contains("file"));
        assert!(checked_url("tauri://localhost").is_err());
        assert!(checked_url("javascript:alert(1)").is_err());
        assert!(checked_url("not a url").unwrap_err().contains("invalid URL"));
    }

    #[test]
    fn labels_are_namespaced() {
        assert_eq!(label("abc"), "browser-abc");
    }
}
