//! Free-port scanning and backend-reachability checks. Two distinct questions the
//! Add-Site UI needs answered separately, per the plan's risk notes: "is this port
//! free" (nothing bound to it — a candidate for a new site) vs. "is my backend
//! actually running there" (something IS bound, and it should be the user's app).

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

pub fn is_port_free(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpListener::bind(addr).is_ok()
}

/// True if something is already accepting connections on this port — i.e. the
/// user's backend (or an unrelated process) is up.
pub fn is_backend_reachable(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// Scans upward from `start` for the first free port, within a bounded window so a
/// pathological "everything is taken" case doesn't hang.
pub fn suggest_free_port(start: u16) -> Option<u16> {
    (start..start.saturating_add(200)).find(|&p| is_port_free(p))
}

#[tauri::command(async)]
pub fn nginx_ssl_suggest_port(start: u16) -> Option<u16> {
    suggest_free_port(start)
}

#[tauri::command(async)]
pub fn nginx_ssl_check_port(port: u16) -> bool {
    is_backend_reachable(port)
}
