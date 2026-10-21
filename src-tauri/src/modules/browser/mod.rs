//! Browser — real web pages inside the app window.
//!
//! An `<iframe>` was the obvious first idea and it doesn't work: most sites
//! (and every site that sends `X-Frame-Options`) refuse to be framed, and the
//! ones that allow it still can't be inspected, zoomed or navigated by us.
//!
//! So each tab is a WebKit widget of its own, hosted inside the app window's
//! GTK tree (see `host.rs`), parked at the rectangle the page's content area
//! occupies. React draws the tabs, address bar and skin; the native view
//! draws the page. The two never overlap by design: the frontend reports the
//! content rectangle with a ResizeObserver and we move the widget to match.
//!
//! What makes it a browser rather than a viewer:
//!   - One shared [`WebContext`] with a data directory under the app's data
//!     dir, so cookies, sessions and local storage survive a restart. Log in
//!     to a site once and it stays logged in, like any browser.
//!   - Web Inspector (`with_devtools`) on every tab: right-click → Inspect
//!     Element, or the Inspect command.
//!   - A user-agent per tab. The engine is always WebKitGTK — there is no way
//!     to embed Blink or Gecko without shipping them — but most "works in
//!     Chrome, breaks in Safari" bugs are UA-sniffing and feature-detection
//!     branches, and a UA switch reaches those. For a true engine check the
//!     frontend can hand the URL to any real browser that is installed
//!     (`browser_system_browsers`).
//!   - Saved logins. An initialization script watches password forms and
//!     posts the submitted pair over wry's IPC; the *origin* is taken from
//!     the IPC request's own URL, never from the page's message, so a page
//!     can't save a password under another site's name. Passwords rest
//!     encrypted (AES-GCM) under a per-install key file with mode 0600. On
//!     load, a site with exactly one saved login is filled in automatically.
//!   - History and bookmarks in the app database, for the address bar's
//!     suggestions and the bookmarks bar.
//!   - Downloads land in the user's Downloads directory and the frontend is
//!     told when they start and finish.
//!
//! A wry `WebView` is a GTK object and must live on the main thread, so the
//! tabs sit in a thread-local there and every command hops over with
//! `run_on_main_thread` and waits on a channel.
//!
//! Consequences worth knowing:
//!   - A native child always paints above the app's DOM, so the page hides
//!     every tab that isn't shown, and hides all of them when the user leaves
//!     the Browser route. Otherwise a web page would sit on top of the Docker
//!     table.
//!   - We can't ask the page "can you go back?" — that would need IPC into a
//!     foreign origin. Back and forward are `history.back()`/`forward()` via
//!     script, and the buttons stay enabled; a no-op click costs nothing.
//!   - Titles and URLs come from wry's own hooks, not the page, so a hostile
//!     page can't spoof the address bar.
//!   - The user agent is fixed at creation in wry, so switching it means the
//!     frontend closes and reopens the view at the same URL. The shared
//!     context keeps the session, so the page comes back logged in.
//!
//! The tab id is client-chosen, for the same reason as PTY sessions: the
//! frontend registers its event listeners before the view exists, so the first
//! title change is never missed.

pub mod credentials;
mod focus;
// Linux hosts each tab in the app window's GTK tree; elsewhere wry parents the
// view to the native window directly (`build_as_child`), so there is no host.
#[cfg(target_os = "linux")]
mod host;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use tauri::{AppHandle, Emitter, Manager, State, Url};
use wry::dpi::{LogicalPosition, LogicalSize};
#[cfg(target_os = "linux")]
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use webkit2gtk::WebViewExt;
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};
#[cfg(target_os = "linux")]
use wry::{WebViewBuilderExtUnix, WebViewExtUnix};

pub const TITLE_EVENT: &str = "browser:title";
pub const LOAD_EVENT: &str = "browser:load";
pub const NEW_WINDOW_EVENT: &str = "browser:new-window";
pub const DOWNLOAD_EVENT: &str = "browser:download";

/// Appended to `db::later_migrations()`; never reordered.
pub const MIGRATION_BROWSER: &str = "
    CREATE TABLE browser_bookmarks (
        id TEXT PRIMARY KEY,
        url TEXT NOT NULL,
        title TEXT NOT NULL,
        position INTEGER NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE browser_history (
        id TEXT PRIMARY KEY,
        url TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL DEFAULT '',
        visits INTEGER NOT NULL DEFAULT 1,
        visited_at TEXT NOT NULL
    );
    CREATE INDEX browser_history_visited ON browser_history(visited_at DESC);
    CREATE TABLE browser_credentials (
        id TEXT PRIMARY KEY,
        origin TEXT NOT NULL,
        username TEXT NOT NULL,
        secret TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(origin, username)
    );
";

thread_local! {
    static TABS: RefCell<HashMap<String, WebView>> = RefCell::new(HashMap::new());
    /// Shared by every tab so they see one cookie jar. Created on first use
    /// and never dropped: a context must outlive the views made from it.
    static CONTEXT: RefCell<Option<WebContext>> = const { RefCell::new(None) };
}

/// Watches password forms and reports the submitted pair. Runs in the main
/// frame only; a login form inside a third-party iframe is not ours to save.
/// Guarded so a page that re-runs scripts can't double-register.
const CAPTURE_SCRIPT: &str = r#"
(function () {
  if (window.__devsproCapture) return;
  window.__devsproCapture = true;
  var last = "";
  function usernameNear(root, pw) {
    var inputs = Array.prototype.slice.call(root.querySelectorAll("input"));
    var at = inputs.indexOf(pw);
    for (var i = at - 1; i >= 0; i--) {
      var t = (inputs[i].getAttribute("type") || "text").toLowerCase();
      if ((t === "text" || t === "email" || t === "tel") && inputs[i].value) return inputs[i].value;
    }
    return "";
  }
  function report(root) {
    if (!root) return;
    var pw = root.querySelector("input[type=password]");
    if (!pw || !pw.value) return;
    var msg = JSON.stringify({ type: "credential", username: usernameNear(root, pw), password: pw.value });
    if (msg === last) return;
    last = msg;
    try { window.ipc.postMessage(msg); } catch (e) {}
  }
  // Tell the host which tab was clicked so GTK focus (keyboard) follows the pointer.
  function wantFocus() { try { window.ipc.postMessage('{"type":"focus"}'); } catch (e) {} }
  document.addEventListener("mousedown", wantFocus, true);
  document.addEventListener("touchstart", wantFocus, true);
  document.addEventListener("submit", function (e) {
    if (e.target && e.target.tagName === "FORM") report(e.target);
  }, true);
  document.addEventListener("click", function (e) {
    var b = e.target && e.target.closest && e.target.closest("button, input[type=submit]");
    if (!b) return;
    report(b.form || b.closest("form") || document.body);
  }, true);
  document.addEventListener("keydown", function (e) {
    if (e.key !== "Enter" || !e.target || e.target.tagName !== "INPUT") return;
    report(e.target.form || document.body);
  }, true);
})();
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<&Bounds> for Rect {
    fn from(b: &Bounds) -> Rect {
        Rect {
            position: LogicalPosition::new(b.x, b.y).into(),
            size: LogicalSize::new(b.width.max(1.0), b.height.max(1.0)).into(),
        }
    }
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

#[derive(Debug, Clone, Serialize)]
struct DownloadEvent {
    id: String,
    url: String,
    path: Option<String>,
    /// "started" or "finished".
    state: &'static str,
    success: bool,
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

/// What a page may navigate itself to. Broader than `checked_url` because
/// pages legitimately use `about:blank`, `blob:` and `data:` URLs internally;
/// still no `file:` and nothing that reaches the app's own origin.
fn navigation_allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("about:")
        || lower.starts_with("blob:http")
        || lower.starts_with("data:")
}

/// "https://www.example.com:8443/a?b" -> "https://www.example.com:8443".
/// The key that logins and cookies are filed under.
pub(crate) fn origin_of(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    Some(match parsed.port() {
        Some(p) => format!("{}://{}:{}", parsed.scheme(), host, p),
        None => format!("{}://{}", parsed.scheme(), host),
    })
}

/// Runs `f` on the GTK main thread and waits for its answer.
fn on_main<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| format!("main thread unavailable: {e}"))?;
    rx.recv()
        .map_err(|_| "the browser view did not answer".to_string())?
}

/// Borrows one tab on the main thread; every command but `open` goes through here.
fn with_tab<T, F>(app: &AppHandle, id: String, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&WebView) -> wry::Result<T> + Send + 'static,
{
    on_main(app, move || {
        TABS.with(|tabs| {
            let tabs = tabs.borrow();
            let wv = tabs
                .get(&id)
                .ok_or_else(|| "that browser tab is no longer open".to_string())?;
            f(wv).map_err(|e| e.to_string())
        })
    })
}

/// Runs a script in a tab from inside one of wry's own callbacks, which
/// already execute on the main thread. `try_borrow` because a callback can in
/// principle fire while `browser_close` holds the map.
pub(crate) fn eval_in_tab(id: &str, script: &str) {
    TABS.with(|tabs| {
        if let Ok(tabs) = tabs.try_borrow() {
            if let Some(wv) = tabs.get(id) {
                let _ = wv.evaluate_script(script);
            }
        }
    });
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The current URL of a tab, from inside a wry callback on the main thread.
fn tab_url(id: &str) -> Option<String> {
    TABS.with(|tabs| tabs.try_borrow().ok().and_then(|t| t.get(id).and_then(|wv| wv.url().ok())))
}

/// History writes happen inside wry's page-load and title callbacks, which
/// run on the GTK main thread — the thread that paints every tab. Taking the
/// app database lock there means a page load waits behind whatever command
/// currently holds it (a dump, a backup, a slow query), and the write itself
/// is disk I/O. So the callbacks hand the write to a worker and return.
fn record_visit(app: &AppHandle, url: &str) {
    if origin_of(url).is_none() {
        return;
    }
    let app = app.clone();
    let url = url.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(db) = app.try_state::<crate::db::Db>() else { return };
        let Ok(conn) = db.0.lock() else { return };
        let _ = conn.execute(
            "INSERT INTO browser_history (id, url, title, visits, visited_at) VALUES (?1, ?2, '', 1, ?3)
             ON CONFLICT(url) DO UPDATE SET visits = visits + 1, visited_at = excluded.visited_at",
            params![uuid::Uuid::new_v4().to_string(), url, now()],
        );
    });
}

fn record_title(app: &AppHandle, url: &str, title: &str) {
    if title.trim().is_empty() {
        return;
    }
    let app = app.clone();
    let (url, title) = (url.to_string(), title.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let Some(db) = app.try_state::<crate::db::Db>() else { return };
        let Ok(conn) = db.0.lock() else { return };
        let _ = conn.execute(
            "UPDATE browser_history SET title = ?1 WHERE url = ?2",
            params![title, url],
        );
    });
}

/// A file name for a download that doesn't clobber one already there:
/// `report.pdf`, then `report (1).pdf`, `report (2).pdf`.
fn unique_download_path(dir: &std::path::Path, suggested: &str) -> PathBuf {
    let name = std::path::Path::new(suggested)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("download");
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("an unused download name exists")
}

#[tauri::command]
pub fn browser_open(
    app: AppHandle,
    window: tauri::Window,
    id: String,
    url: String,
    bounds: Bounds,
    user_agent: Option<String>,
) -> Result<(), String> {
    let result = browser_open_inner(app, window, id, url, bounds, user_agent);
    if let Err(e) = &result {
        // The frontend shows this in a banner, but a terminal is where a
        // developer looks first when a tab refuses to appear.
        eprintln!("browser: open failed: {e}");
    }
    result
}

fn browser_open_inner(
    app: AppHandle,
    window: tauri::Window,
    id: String,
    url: String,
    bounds: Bounds,
    user_agent: Option<String>,
) -> Result<(), String> {
    let url = checked_url(&url)?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("browser");
    let downloads_dir = app.path().download_dir().ok();
    // The window that asked, not "main": the Browser module can be popped out
    // into its own window, and the child view must land in that one. Events
    // go back to the same window so a second window's Browser page never
    // reacts to this one's tabs.
    let label = window.label().to_string();
    let handle = app.clone();

    on_main(&handle, move || {
        let exists = TABS.with(|tabs| tabs.borrow().contains_key(&id));
        if exists {
            return Err("that tab already exists".into());
        }
        let webview = CONTEXT.with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            let ctx = ctx.get_or_insert_with(|| WebContext::new(Some(data_dir)));

            let (title_app, title_label, title_id) = (app.clone(), label.clone(), id.clone());
            let (load_app, load_label, load_id) = (app.clone(), label.clone(), id.clone());
            let (new_app, new_label, new_id) = (app.clone(), label.clone(), id.clone());
            let (ipc_app, ipc_label, ipc_id) = (app.clone(), label.clone(), id.clone());
            let (dl_app, dl_label, dl_id) = (app.clone(), label.clone(), id.clone());
            let (done_app, done_label, done_id) = (app.clone(), label.clone(), id.clone());

            let mut builder = WebViewBuilder::new_with_web_context(ctx)
                .with_url(url.as_str())
                .with_bounds(Rect::from(&bounds))
                .with_devtools(true)
                .with_clipboard(true)
                .with_hotkeys_zoom(true)
                .with_back_forward_navigation_gestures(true)
                .with_initialization_script(CAPTURE_SCRIPT)
                .with_navigation_handler(|url| navigation_allowed(&url))
                .with_document_title_changed_handler(move |title| {
                    if let Some(url) = tab_url(&title_id) {
                        record_title(&title_app, &url, &title);
                    }
                    let _ = title_app.emit_to(
                        &title_label,
                        TITLE_EVENT,
                        TitleEvent { id: title_id.clone(), title },
                    );
                })
                .with_on_page_load_handler(move |event, url| {
                    let state = match event {
                        PageLoadEvent::Started => "started",
                        PageLoadEvent::Finished => "finished",
                    };
                    if matches!(event, PageLoadEvent::Finished) {
                        record_visit(&load_app, &url);
                        credentials::autofill(&load_app, &load_id, &url);
                    }
                    let _ = load_app.emit_to(
                        &load_label,
                        LOAD_EVENT,
                        LoadEvent { id: load_id.clone(), url, state },
                    );
                })
                // target=_blank and window.open become a new tab in our strip
                // instead of a bare OS window with no chrome at all.
                .with_new_window_req_handler(move |url, _features| {
                    let _ = new_app.emit_to(
                        &new_label,
                        NEW_WINDOW_EVENT,
                        NewWindowEvent { id: new_id.clone(), url },
                    );
                    NewWindowResponse::Deny
                })
                .with_ipc_handler(move |request| {
                    if request.body().trim() == r#"{"type":"focus"}"# {
                        TABS.with(|tabs| {
                            if let Ok(tabs) = tabs.try_borrow() {
                                if let Some(wv) = tabs.get(&ipc_id) {
                                    let _ = focus::focus_child(wv);
                                }
                            }
                        });
                        return;
                    }
                    // The URI on the request is wry's own record of which page
                    // sent it — the one thing in this message a page can't lie about.
                    let page_url = request.uri().to_string();
                    credentials::on_ipc(&ipc_app, &ipc_label, &ipc_id, &page_url, request.body());
                })
                .with_download_completed_handler(move |url, path, success| {
                    let _ = done_app.emit_to(
                        &done_label,
                        DOWNLOAD_EVENT,
                        DownloadEvent {
                            id: done_id.clone(),
                            url,
                            path: path.map(|p| p.display().to_string()),
                            state: "finished",
                            success,
                        },
                    );
                });

            if let Some(dir) = downloads_dir {
                builder = builder.with_download_started_handler(move |url, path| {
                    let suggested = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    let fallback = url.rsplit('/').next().unwrap_or("download").to_string();
                    let name = if suggested.is_empty() { fallback } else { suggested };
                    let _ = std::fs::create_dir_all(&dir);
                    *path = unique_download_path(&dir, &name);
                    let _ = dl_app.emit_to(
                        &dl_label,
                        DOWNLOAD_EVENT,
                        DownloadEvent {
                            id: dl_id.clone(),
                            url,
                            path: Some(path.display().to_string()),
                            state: "started",
                            success: true,
                        },
                    );
                    true
                });
            }
            if let Some(ua) = user_agent.filter(|ua| !ua.trim().is_empty()) {
                builder = builder.with_user_agent(ua);
            }

            #[cfg(target_os = "linux")]
            let wv = {
                let fixed = host::container_for(&window)?;
                let wv = builder
                    .build_gtk(&fixed)
                    .map_err(|e| format!("could not create the browser view: {e}"))?;
                host::place(&fixed, wv.webview().upcast_ref(), bounds.x, bounds.y, bounds.width, bounds.height);
                wv.webview().show_all();
                wv
            };
            // A native child window (WebView2 on Windows) positioned by `with_bounds`.
            #[cfg(not(target_os = "linux"))]
            let wv = builder
                .build_as_child(&window)
                .map_err(|e| format!("could not create the browser view: {e}"))?;
            Ok::<WebView, String>(wv)
        })?;
        TABS.with(|tabs| tabs.borrow_mut().insert(id, webview));
        Ok(())
    })
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let url = checked_url(&url)?;
    with_tab(&app, id, move |wv| wv.load_url(url.as_str()))
}

#[tauri::command]
pub fn browser_back(app: AppHandle, id: String) -> Result<(), String> {
    with_tab(&app, id, |wv| wv.evaluate_script("history.back()"))
}

#[tauri::command]
pub fn browser_forward(app: AppHandle, id: String) -> Result<(), String> {
    with_tab(&app, id, |wv| wv.evaluate_script("history.forward()"))
}

#[tauri::command]
pub fn browser_reload(app: AppHandle, id: String) -> Result<(), String> {
    with_tab(&app, id, |wv| wv.reload())
}

#[tauri::command]
pub fn browser_stop(app: AppHandle, id: String) -> Result<(), String> {
    with_tab(&app, id, |wv| wv.evaluate_script("window.stop()"))
}

#[tauri::command]
pub fn browser_set_bounds(app: AppHandle, id: String, bounds: Bounds) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return with_tab(&app, id, move |wv| {
        let widget = wv.webview();
        if let Some(fixed) = widget.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
            host::place(&fixed, widget.upcast_ref(), bounds.x, bounds.y, bounds.width, bounds.height);
        }
        Ok(())
    });
    #[cfg(not(target_os = "linux"))]
    return with_tab(&app, id, move |wv| wv.set_bounds(Rect::from(&bounds)));
}

#[tauri::command]
pub fn browser_set_visible(app: AppHandle, id: String, visible: bool) -> Result<(), String> {
    with_tab(&app, id, move |wv| wv.set_visible(visible))
}

#[tauri::command]
pub fn browser_set_zoom(app: AppHandle, id: String, factor: f64) -> Result<(), String> {
    with_tab(&app, id, move |wv| wv.zoom(factor.clamp(0.25, 5.0)))
}

/// Keyboard into a tab's page: GTK focus on its WebKit widget. See `focus` for why this is more than
/// `grab_focus`.
#[tauri::command]
pub fn browser_focus(app: AppHandle, id: String) -> Result<(), String> {
    on_main(&app, move || {
        TABS.with(|tabs| {
            let tabs = tabs.borrow();
            let wv = tabs
                .get(&id)
                .ok_or_else(|| "that browser tab is no longer open".to_string())?;
            focus::focus_child(wv)
        })
    })
}

/// Keyboard back to the app's own UI: the address bar, the tab strip, any
/// dialog. The frontend calls this on every click outside a page.
#[tauri::command]
pub fn browser_focus_parent(app: AppHandle, window: tauri::Window) -> Result<(), String> {
    on_main(&app, move || {
        #[cfg(not(target_os = "linux"))]
        TABS.with(|tabs| {
            if let Ok(tabs) = tabs.try_borrow() {
                if let Some(wv) = tabs.values().next() {
                    let _ = wv.focus_parent();
                }
            }
        });
        focus::focus_app_window(&window)
    })
}

/// Web Inspector for one tab. WebKitGTK opens it in its own window, docked or
/// not as the user last left it.
#[tauri::command]
pub fn browser_devtools(app: AppHandle, id: String, open: bool) -> Result<(), String> {
    with_tab(&app, id, move |wv| {
        if open {
            wv.open_devtools();
        } else {
            wv.close_devtools();
        }
        Ok(())
    })
}

#[tauri::command]
pub fn browser_print(app: AppHandle, id: String) -> Result<(), String> {
    with_tab(&app, id, |wv| wv.print())
}

/// Find in page, using the engine's own `window.find`. Wraps, case-insensitive.
/// An empty query clears the selection so the highlight goes away.
#[tauri::command]
pub fn browser_find(app: AppHandle, id: String, text: String, backwards: bool) -> Result<(), String> {
    let script = if text.is_empty() {
        "window.getSelection().removeAllRanges()".to_string()
    } else {
        format!(
            "window.find({}, false, {}, true, false, true, false)",
            serde_json::to_string(&text).unwrap_or_default(),
            backwards
        )
    };
    with_tab(&app, id, move |wv| wv.evaluate_script(&script))
}

/// Cookies, local storage, caches — the lot, for every tab, since they share
/// one context. Needs at least one open tab to reach the context through.
#[tauri::command]
pub fn browser_clear_data(app: AppHandle) -> Result<(), String> {
    on_main(&app, || {
        TABS.with(|tabs| {
            let tabs = tabs.borrow();
            let wv = tabs
                .values()
                .next()
                .ok_or_else(|| "open a tab first, then clear browsing data".to_string())?;
            wv.clear_all_browsing_data().map_err(|e| e.to_string())
        })
    })
}

/// Dropping the `WebView` destroys its widget. Closing a tab that
/// already went away is fine: the strip and the native side can disagree
/// briefly during teardown.
#[tauri::command]
pub fn browser_close(app: AppHandle, id: String) -> Result<(), String> {
    on_main(&app, move || {
        TABS.with(|tabs| {
            if let Some(wv) = tabs.borrow_mut().remove(&id) {
                let _ = wv.set_visible(false);
                // Unparenting the widget is not enough: signal closures keep
                // the WebKit view alive, and with it the page — audio kept
                // playing and the remote inspector kept listing closed tabs.
                // Closing the page itself ends it regardless of the widget.
                #[cfg(target_os = "linux")]
                {
                    let widget = wv.webview();
                    widget.stop_loading();
                    widget.try_close();
                    host::remove(widget.upcast_ref());
                }
                // Elsewhere dropping the WebView tears down the native child.
                drop(wv);
            }
        });
        Ok(())
    })
}

/// Every open tab, so a remounted page (the user came back to the route) can
/// re-adopt views that survived while it was away.
#[tauri::command]
pub fn browser_list(app: AppHandle) -> Result<Vec<String>, String> {
    on_main(&app, || Ok(TABS.with(|tabs| tabs.borrow().keys().cloned().collect())))
}

// ---------------------------------------------------------------- bookmarks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub position: i64,
    pub created_at: String,
}

#[tauri::command(async)]
pub fn browser_bookmarks_list(db: State<crate::db::Db>) -> Result<Vec<Bookmark>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, url, title, position, created_at FROM browser_bookmarks ORDER BY position, created_at")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Bookmark {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                position: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn browser_bookmark_add(db: State<crate::db::Db>, url: String, title: String) -> Result<Bookmark, String> {
    checked_url(&url)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let position: i64 = conn
        .query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM browser_bookmarks", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let bookmark = Bookmark {
        id: uuid::Uuid::new_v4().to_string(),
        url,
        title: if title.trim().is_empty() { "Untitled".into() } else { title.trim().to_string() },
        position,
        created_at: now(),
    };
    conn.execute(
        "INSERT INTO browser_bookmarks (id, url, title, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![bookmark.id, bookmark.url, bookmark.title, bookmark.position, bookmark.created_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(bookmark)
}

#[tauri::command(async)]
pub fn browser_bookmark_update(
    db: State<crate::db::Db>,
    id: String,
    url: String,
    title: String,
) -> Result<(), String> {
    checked_url(&url)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE browser_bookmarks SET url = ?1, title = ?2 WHERE id = ?3",
        params![url, title.trim(), id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn browser_bookmark_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM browser_bookmarks WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Drag-to-reorder sends the whole order back; positions are rewritten 0..n.
#[tauri::command(async)]
pub fn browser_bookmarks_reorder(db: State<crate::db::Db>, ids: Vec<String>) -> Result<(), String> {
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute("UPDATE browser_bookmarks SET position = ?1 WHERE id = ?2", params![i as i64, id])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ history

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub url: String,
    pub title: String,
    pub visits: i64,
    pub visited_at: String,
}

fn history_rows(stmt: &mut rusqlite::Statement, p: &[&dyn rusqlite::ToSql]) -> Result<Vec<HistoryEntry>, String> {
    let rows = stmt
        .query_map(p, |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                visits: r.get(3)?,
                visited_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Address-bar suggestions: substring match on URL or title, most visited
/// first, then most recent. An empty query lists the recent pages instead.
#[tauri::command(async)]
pub fn browser_history_search(
    db: State<crate::db::Db>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(8).min(200) as i64;
    let q = query.trim();
    if q.is_empty() {
        let mut stmt = conn
            .prepare("SELECT id, url, title, visits, visited_at FROM browser_history ORDER BY visited_at DESC LIMIT ?1")
            .map_err(|e| e.to_string())?;
        return history_rows(&mut stmt, &[&limit]);
    }
    let pattern = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let mut stmt = conn
        .prepare(
            "SELECT id, url, title, visits, visited_at FROM browser_history
             WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY visits DESC, visited_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    history_rows(&mut stmt, &[&pattern, &limit])
}

#[tauri::command(async)]
pub fn browser_history_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM browser_history WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn browser_history_clear(db: State<crate::db::Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM browser_history", []).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------- system browsers

#[derive(Debug, Clone, Serialize)]
pub struct SystemBrowser {
    pub id: String,
    pub label: String,
    pub command: String,
}

/// Real engines installed on this machine, for the moments a UA switch isn't
/// enough. Only ones actually on PATH are listed.
#[cfg(not(windows))]
const KNOWN_BROWSERS: &[(&str, &str, &[&str])] = &[
    ("chrome", "Google Chrome", &["google-chrome", "google-chrome-stable", "google-chrome-beta"]),
    ("chromium", "Chromium", &["chromium", "chromium-browser"]),
    ("brave", "Brave", &["brave-browser", "brave"]),
    ("edge", "Microsoft Edge", &["microsoft-edge", "microsoft-edge-stable"]),
    ("firefox", "Firefox", &["firefox", "firefox-esr"]),
    ("epiphany", "GNOME Web", &["epiphany"]),
];

#[cfg(not(windows))]
fn on_path(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn browser_system_browsers() -> Vec<SystemBrowser> {
    KNOWN_BROWSERS
        .iter()
        .filter_map(|(id, label, cmds)| {
            cmds.iter().find(|c| on_path(c)).map(|c| SystemBrowser {
                id: id.to_string(),
                label: label.to_string(),
                command: c.to_string(),
            })
        })
        .collect()
}

/// Windows browsers don't put themselves on PATH, so the Linux lookup finds none of them.
/// Each is looked for where its installer puts it — per-machine under Program Files or
/// per-user under LocalAppData — and then through its `App Paths` registration, which is
/// how Windows itself finds `msedge.exe` from the Run box wherever it was installed.
/// `command` is the full path to the executable.
#[cfg(windows)]
const WINDOWS_BROWSERS: &[(&str, &str, &str, Option<&str>)] = &[
    ("chrome", "Google Chrome", r"Google\Chrome\Application\chrome.exe", Some("chrome.exe")),
    // Chromium's executable is also chrome.exe, so its App Paths entry would be Chrome's.
    ("chromium", "Chromium", r"Chromium\Application\chrome.exe", None),
    ("brave", "Brave", r"BraveSoftware\Brave-Browser\Application\brave.exe", Some("brave.exe")),
    ("edge", "Microsoft Edge", r"Microsoft\Edge\Application\msedge.exe", Some("msedge.exe")),
    ("firefox", "Firefox", r"Mozilla Firefox\firefox.exe", Some("firefox.exe")),
];

#[cfg(windows)]
#[tauri::command(async)]
pub fn browser_system_browsers() -> Vec<SystemBrowser> {
    let roots: Vec<std::path::PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|var| std::env::var_os(var).map(std::path::PathBuf::from))
        .collect();
    WINDOWS_BROWSERS
        .iter()
        .filter_map(|(id, label, relative, app_path)| {
            let found = roots
                .iter()
                .map(|root| root.join(relative))
                .find(|path| path.is_file())
                .or_else(|| app_path.and_then(registered_app_path))?;
            Some(SystemBrowser {
                id: id.to_string(),
                label: label.to_string(),
                command: found.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

/// The executable `exe` is registered at under `App Paths`, per-user first, if that file
/// still exists.
#[cfg(windows)]
fn registered_app_path(exe: &str) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let subkey: Vec<u16> = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = [0u16; 1024];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: `subkey` is NUL-terminated, a null value name reads the key's default
        // value, and `size` tells the call how many bytes `buf` holds.
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            continue;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        let value = String::from_utf16_lossy(&buf[..len]);
        let path = std::path::PathBuf::from(value.trim().trim_matches('"'));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[tauri::command(async)]
pub fn browser_open_in(browser: String, url: String) -> Result<(), String> {
    let url = checked_url(&url)?;
    let found = browser_system_browsers()
        .into_iter()
        .find(|b| b.id == browser)
        .ok_or_else(|| format!("{browser} is not installed"))?;
    crate::modules::tools::command(found.command)
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start {}: {e}", found.label))
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
    fn pages_may_navigate_to_web_and_internal_urls_only() {
        assert!(navigation_allowed("https://example.com"));
        assert!(navigation_allowed("about:blank"));
        assert!(navigation_allowed("blob:https://example.com/abc"));
        assert!(navigation_allowed("data:text/html,hi"));
        assert!(!navigation_allowed("file:///etc/passwd"));
        assert!(!navigation_allowed("tauri://localhost/index.html"));
    }

    #[test]
    fn origin_keeps_scheme_host_and_explicit_port() {
        assert_eq!(origin_of("https://www.example.com/a?b").as_deref(), Some("https://www.example.com"));
        assert_eq!(origin_of("http://localhost:3000/login").as_deref(), Some("http://localhost:3000"));
        assert_eq!(origin_of("about:blank"), None);
        assert_eq!(origin_of("garbage"), None);
    }

    #[test]
    fn bounds_never_collapse_to_zero() {
        let rect = Rect::from(&Bounds { x: 10.0, y: 20.0, width: 0.0, height: -5.0 });
        let size = rect.size.to_logical::<f64>(1.0);
        assert_eq!((size.width, size.height), (1.0, 1.0));
    }

    #[test]
    fn download_names_do_not_clobber() {
        let dir = std::env::temp_dir().join(format!("devspro-dl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_download_path(&dir, "report.pdf"), dir.join("report.pdf"));
        std::fs::write(dir.join("report.pdf"), b"x").unwrap();
        assert_eq!(unique_download_path(&dir, "report.pdf"), dir.join("report (1).pdf"));
        std::fs::write(dir.join("report (1).pdf"), b"x").unwrap();
        assert_eq!(unique_download_path(&dir, "report.pdf"), dir.join("report (2).pdf"));
        assert_eq!(unique_download_path(&dir, "../../evil"), dir.join("evil"));
        assert_eq!(unique_download_path(&dir, ""), dir.join("download"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn history_and_bookmarks_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_BROWSER).unwrap();
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO browser_history (id, url, title, visits, visited_at) VALUES (?1, ?2, '', 1, ?3)
                 ON CONFLICT(url) DO UPDATE SET visits = visits + 1, visited_at = excluded.visited_at",
                params![uuid::Uuid::new_v4().to_string(), "https://a.test/", now()],
            )
            .unwrap();
        }
        let visits: i64 = conn
            .query_row("SELECT visits FROM browser_history WHERE url = 'https://a.test/'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(visits, 2, "a repeat visit bumps the count instead of adding a row");
    }
}
