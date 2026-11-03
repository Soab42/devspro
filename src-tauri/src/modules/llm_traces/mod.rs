//! LLM Traces: a local stand-in for Langfuse and LangSmith, in the way the
//! Mail Catcher is one for SMTP. Point an LLM app's SDK at it and every trace
//! lands on this machine, viewable in DevsPro, with no account, no cloud and
//! nothing leaving the laptop.
//!
//! - `server` is the collector: an in-process HTTP server speaking each SDK's
//!   wire protocol (Langfuse ingestion and OTLP, LangSmith runs, plain OTLP).
//! - `langfuse`, `langsmith` and `otel` translate those protocols into one
//!   model (`model`), which `store` merges into a SQLite file of its own.
//! - The commands below start and stop the collector and read traces back.
//!
//! It runs inside DevsPro, so it lives and dies with the app: off until the
//! user presses Start (or turns on "start with DevsPro"), stopped on quit.
//! It listens on loopback and, by default, on the Docker bridges, so apps in
//! containers can reach it as `host.docker.internal` (see `platform`).

mod langfuse;
mod langsmith;
mod model;
mod otel;
mod platform;
mod server;
mod store;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use store::{ListQuery, Store, StoreStats, TraceDetail, TraceList};
use tauri::{AppHandle, Emitter, Manager, State};

pub use server::RequestLog;

/// Sent, at most every `EMIT_EVERY`, after traces arrive, so an open page
/// refreshes without polling. Bursts of spans collapse into one event.
pub const INGESTED_EVENT: &str = "llm-traces://ingested";
const EMIT_EVERY: Duration = Duration::from_millis(750);

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Settings {
    /// 4318 is OTLP/HTTP's registered port, so an app exporting OpenTelemetry
    /// with default settings reaches the collector with no configuration.
    port: u16,
    start_with_app: bool,
    /// Oldest traces are deleted beyond this.
    max_traces: u32,
    /// Also listen on the Docker bridges, so containers can post traces.
    /// On by default: apps under `docker compose` are the common case, and
    /// the collector answers only containers there (see `server`).
    docker: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { port: 4318, start_with_app: false, max_traces: 2000, docker: true }
    }
}

const MAX_TRACES_RANGE: (u32, u32) = (100, 50_000);

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("llm-traces");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn load_settings(dir: &Path) -> Settings {
    std::fs::read_to_string(dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("settings.json"), text).map_err(|e| format!("could not save LLM Traces settings: {e}"))
}

#[derive(Default)]
pub struct LlmTracesState {
    /// Opened on first use and kept for the life of the app, running or not:
    /// the page reads stored traces while the collector is off too.
    ctx: Mutex<Option<Arc<server::Ctx>>>,
    /// A tokio mutex because starting awaits the bind; it also serialises
    /// start and stop, so two clicks can't race for the port.
    running: tokio::sync::Mutex<Option<server::ServerHandle>>,
}

fn ctx(app: &AppHandle) -> Result<Arc<server::Ctx>, String> {
    let state = app.state::<LlmTracesState>();
    let mut slot = state.ctx.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(ctx) = slot.as_ref() {
        return Ok(ctx.clone());
    }
    let dir = data_dir(app)?;
    let store = Arc::new(Store::open(&dir.join("traces.db"))?);
    let dirty = Arc::new(AtomicBool::new(false));
    let flag = dirty.clone();
    let ctx = Arc::new(server::Ctx::new(
        store,
        load_settings(&dir).max_traces,
        Box::new(move |_| flag.store(true, Ordering::Relaxed)),
    ));
    let emitter = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(EMIT_EVERY);
        loop {
            tick.tick().await;
            if dirty.swap(false, Ordering::Relaxed) {
                let _ = emitter.emit(INGESTED_EVENT, ());
            }
        }
    });
    *slot = Some(ctx.clone());
    Ok(ctx)
}

#[derive(Debug, Serialize)]
pub struct LlmTracesStatus {
    pub running: bool,
    /// The port it's on when running; the one it will use when not.
    pub port: u16,
    pub base_url: String,
    /// Something else holds the port, while ours is off.
    pub conflict: Option<String>,
    pub start_with_app: bool,
    pub max_traces: u32,
    /// The Docker setting, and the bridge addresses it is listening on now.
    pub docker: bool,
    pub docker_addresses: Vec<String>,
    pub stats: StoreStats,
}

async fn status(app: &AppHandle, state: &LlmTracesState) -> Result<LlmTracesStatus, String> {
    let dir = data_dir(app)?;
    let settings = load_settings(&dir);
    let (running_port, docker_addresses) = match state.running.lock().await.as_ref() {
        Some(h) => (Some(h.port), h.extra_addrs().iter().map(ToString::to_string).collect()),
        None => (None, Vec::new()),
    };
    let port = running_port.unwrap_or(settings.port);
    let conflict = if running_port.is_none() {
        tauri::async_runtime::spawn_blocking(move || port_in_use(port)).await.unwrap_or(None)
    } else {
        None
    };
    let stats = ctx(app)?.store.stats()?;
    Ok(LlmTracesStatus {
        running: running_port.is_some(),
        port,
        base_url: format!("http://localhost:{port}"),
        conflict,
        start_with_app: settings.start_with_app,
        max_traces: settings.max_traces,
        docker: settings.docker,
        docker_addresses,
        stats,
    })
}

/// "Port 4318 is in use by otelcol (pid 812)." when something else listens there.
fn port_in_use(port: u16) -> Option<String> {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    Some(format!("Port {port} is in use by {}.", owner_of(port)))
}

fn owner_of(port: u16) -> String {
    let entries = crate::modules::processes::listening_ports().unwrap_or_default();
    let mut owners: Vec<String> = Vec::new();
    for entry in entries.iter().filter(|e| e.port == port) {
        let label = match (&entry.process_name, entry.pid) {
            (Some(name), Some(pid)) => format!("{name} (pid {pid})"),
            (None, Some(pid)) => format!("pid {pid}"),
            (Some(name), None) => name.clone(),
            (None, None) => continue,
        };
        if !owners.contains(&label) {
            owners.push(label);
        }
    }
    if owners.is_empty() {
        "another program".to_string()
    } else {
        owners.join(", ")
    }
}

/// Starts on `port`, or restarts there when it's already running elsewhere,
/// or `restart` asks (the Docker setting changed).
async fn start_inner(app: &AppHandle, port: u16, restart: bool) -> Result<(), String> {
    if port == 0 {
        return Err("the port must be between 1 and 65535".into());
    }
    let state = app.state::<LlmTracesState>();
    let mut running = state.running.lock().await;
    if let Some(current) = running.as_ref() {
        if current.port == port && !restart {
            return Ok(());
        }
    }
    let restarting = running.take().map(server::ServerHandle::stop).is_some();
    let ctx = ctx(app)?;
    let extra: Option<server::ExtraAddrs> =
        load_settings(&data_dir(app)?).docker.then_some(platform::container_networks as server::ExtraAddrs);
    // Our own listeners close on their next poll, so a restart may find the
    // port still held by them for a moment: retry for up to a second then.
    let mut attempts = if restarting { 20 } else { 1 };
    let handle = loop {
        match server::start(port, ctx.clone(), extra).await {
            Ok(handle) => break handle,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempts > 1 => {
                attempts -= 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(format!("Port {port} is in use by {}. Pick another port.", owner_of(port)))
            }
            Err(e) => return Err(format!("could not listen on port {port}: {e}")),
        }
    };
    *running = Some(handle);
    drop(running);

    let dir = data_dir(app)?;
    let mut settings = load_settings(&dir);
    settings.port = port;
    save_settings(&dir, &settings)
}

#[tauri::command]
pub async fn llm_traces_status(app: AppHandle, state: State<'_, LlmTracesState>) -> Result<LlmTracesStatus, String> {
    status(&app, &state).await
}

#[tauri::command]
pub async fn llm_traces_start(app: AppHandle, state: State<'_, LlmTracesState>, port: u16) -> Result<LlmTracesStatus, String> {
    start_inner(&app, port, false).await?;
    status(&app, &state).await
}

#[tauri::command]
pub async fn llm_traces_stop(app: AppHandle, state: State<'_, LlmTracesState>) -> Result<LlmTracesStatus, String> {
    if let Some(handle) = state.running.lock().await.take() {
        handle.stop();
    }
    status(&app, &state).await
}

#[tauri::command]
pub async fn llm_traces_set_options(
    app: AppHandle,
    state: State<'_, LlmTracesState>,
    start_with_app: bool,
    max_traces: u32,
    docker: bool,
) -> Result<LlmTracesStatus, String> {
    let dir = data_dir(&app)?;
    let mut settings = load_settings(&dir);
    let docker_changed = settings.docker != docker;
    settings.start_with_app = start_with_app;
    settings.max_traces = max_traces.clamp(MAX_TRACES_RANGE.0, MAX_TRACES_RANGE.1);
    settings.docker = docker;
    save_settings(&dir, &settings)?;
    ctx(&app)?.max_traces.store(settings.max_traces, Ordering::Relaxed);
    // Which addresses it listens on is decided at start, so apply it now.
    let running_port = state.running.lock().await.as_ref().map(|h| h.port);
    if let (true, Some(port)) = (docker_changed, running_port) {
        start_inner(&app, port, true).await?;
    }
    status(&app, &state).await
}

#[tauri::command(async)]
pub fn llm_traces_list(app: AppHandle, query: ListQuery) -> Result<TraceList, String> {
    ctx(&app)?.store.list(&query)
}

#[tauri::command(async)]
pub fn llm_traces_get(app: AppHandle, id: String) -> Result<Option<TraceDetail>, String> {
    ctx(&app)?.store.get(&id)
}

#[tauri::command(async)]
pub fn llm_traces_delete(app: AppHandle, ids: Vec<String>) -> Result<usize, String> {
    ctx(&app)?.store.delete(&ids)
}

#[tauri::command(async)]
pub fn llm_traces_clear(app: AppHandle) -> Result<(), String> {
    ctx(&app)?.store.clear()
}

/// The collector's last requests, newest first, for the Connect tab.
#[tauri::command(async)]
pub fn llm_traces_recent(app: AppHandle) -> Result<Vec<RequestLog>, String> {
    Ok(ctx(&app)?.recent())
}

/// At launch: start the collector if the user asked for that. A port taken
/// by something else is only logged; the page shows the conflict.
pub fn on_startup(app: &AppHandle) {
    let Ok(dir) = data_dir(app) else { return };
    let settings = load_settings(&dir);
    if !settings.start_with_app {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_inner(&app, settings.port, false).await {
            eprintln!("llm traces: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_off_on_the_otlp_port() {
        let s = Settings::default();
        assert_eq!((s.port, s.start_with_app, s.max_traces, s.docker), (4318, false, 2000, true));
        let older: Settings = serde_json::from_str("{\"port\": 9000}").unwrap();
        assert_eq!((older.port, older.max_traces, older.docker), (9000, 2000, true));
    }
}
