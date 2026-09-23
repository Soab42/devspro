//! The WebSocket-to-TCP bridge behind the in-app VNC viewer.
//!
//! noVNC runs in the webview and can only open WebSockets, while a VNC server
//! listens on a plain TCP port. `remote_desktop_vnc_open` therefore binds a
//! loopback listener on a random port and returns its URL; every WebSocket the
//! frontend opens on it is connected to the server and the two are piped
//! byte-for-byte in both directions (websockify, in fifty lines). The URL
//! carries a random token in its path and the listener rejects anything else,
//! so another local process guessing the port cannot borrow the session.
//!
//! Nothing here understands RFB. Authentication, encodings and input all
//! happen between noVNC and the server; this file only moves bytes and
//! reports, over `remote-desktop:vnc` events, the one thing noVNC can't see —
//! why the TCP side failed to connect.

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;

pub const BRIDGE_EVENT: &str = "remote-desktop:vnc";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Bridge {
    accept: tauri::async_runtime::JoinHandle<()>,
    /// Flipped to `true` on close; every pipe task watches it.
    cancel: watch::Sender<bool>,
}

pub type BridgeMap = Mutex<HashMap<String, Bridge>>;

#[derive(Debug, Clone, Serialize)]
pub struct VncBridge {
    pub session: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
struct BridgeEvent {
    session: String,
    /// "connected" | "error" | "closed"
    kind: &'static str,
    message: String,
}

/// How a bridge reports what happened: an app event in production, a
/// recording closure in tests. Takes the kind and the message.
type Reporter = Arc<dyn Fn(&'static str, String) + Send + Sync>;

fn event_reporter(app: AppHandle, session: String) -> Reporter {
    Arc::new(move |kind, message| {
        let _ = app.emit(BRIDGE_EVENT, BridgeEvent { session: session.clone(), kind, message });
    })
}

/// Bind the loopback listener for one saved VNC connection and return where
/// the viewer should connect. The server is only dialled when the viewer
/// actually opens the WebSocket, so a stale bridge costs nothing.
#[tauri::command]
pub async fn remote_desktop_vnc_open(
    app: AppHandle,
    db: State<'_, crate::db::Db>,
    state: State<'_, super::RemoteDesktopState>,
    id: String,
) -> Result<VncBridge, String> {
    let desktop = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::connections::find(&conn, &id)?
    };
    if desktop.protocol != "vnc" {
        return Err("this connection is not a VNC connection".to_string());
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("failed to open a local port for the viewer: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let session = uuid::Uuid::new_v4().simple().to_string();
    let url = format!("ws://127.0.0.1:{port}/{session}");

    let (cancel, cancelled) = watch::channel(false);
    let accept = tauri::async_runtime::spawn(accept_loop(
        event_reporter(app, session.clone()),
        listener,
        format!("/{session}"),
        desktop.host,
        desktop.port,
        cancelled,
    ));

    state
        .vnc
        .lock()
        .map_err(|e| e.to_string())?
        .insert(session.clone(), Bridge { accept, cancel });
    Ok(VncBridge { session, url })
}

#[tauri::command]
pub fn remote_desktop_vnc_close(
    state: State<'_, super::RemoteDesktopState>,
    session: String,
) -> Result<(), String> {
    let bridge = state.vnc.lock().map_err(|e| e.to_string())?.remove(&session);
    if let Some(bridge) = bridge {
        stop(bridge);
    }
    Ok(())
}

fn stop(bridge: Bridge) {
    let _ = bridge.cancel.send(true);
    bridge.accept.abort();
}

/// Tear down every bridge — the app is exiting.
pub fn kill_all(state: &super::RemoteDesktopState) {
    if let Ok(mut map) = state.vnc.lock() {
        for (_, bridge) in map.drain() {
            stop(bridge);
        }
    }
}

async fn accept_loop(
    report: Reporter,
    listener: TcpListener,
    expected_path: String,
    host: String,
    port: u16,
    cancelled: watch::Receiver<bool>,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { return };
        tauri::async_runtime::spawn(serve(
            report.clone(),
            stream,
            expected_path.clone(),
            host.clone(),
            port,
            cancelled.clone(),
        ));
    }
}

/// One accepted socket: handshake, dial the server, pipe until either side
/// hangs up or the bridge is closed.
async fn serve(
    report: Reporter,
    stream: TcpStream,
    expected_path: String,
    host: String,
    port: u16,
    mut cancelled: watch::Receiver<bool>,
) {
    let check = |req: &Request, mut res: Response| -> Result<Response, ErrorResponse> {
        if req.uri().path() != expected_path {
            let mut not_found = ErrorResponse::new(None);
            *not_found.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
            return Err(not_found);
        }
        // noVNC may ask for the "binary" subprotocol; echo the first one it
        // offers so the browser accepts the handshake either way.
        if let Some(proto) = req.headers().get("Sec-WebSocket-Protocol") {
            if let Ok(first) = proto.to_str() {
                if let Some(first) = first.split(',').next() {
                    if let Ok(value) = first.trim().parse() {
                        res.headers_mut().insert("Sec-WebSocket-Protocol", value);
                    }
                }
            }
        }
        Ok(res)
    };
    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, check).await else { return };

    let target = format!("{host}:{port}");
    let dial = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port)));
    let mut tcp = match dial.await {
        Ok(Ok(tcp)) => tcp,
        Ok(Err(e)) => {
            report("error", format!("could not connect to {target}: {e}"));
            let _ = ws.close(None).await;
            return;
        }
        Err(_) => {
            report("error", format!("timed out connecting to {target}"));
            let _ = ws.close(None).await;
            return;
        }
    };
    let _ = tcp.set_nodelay(true);
    report("connected", target.clone());

    let mut buf = vec![0u8; 64 * 1024];
    let outcome: Result<(), String> = loop {
        tokio::select! {
            _ = cancelled.changed() => break Ok(()),
            msg = ws.next() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Err(e) = tcp.write_all(&bytes).await {
                        break Err(format!("write to {target} failed: {e}"));
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if let Err(e) = tcp.write_all(text.as_bytes()).await {
                        break Err(format!("write to {target} failed: {e}"));
                    }
                }
                Some(Ok(Message::Close(_))) | None => break Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => break Err(format!("viewer socket failed: {e}")),
            },
            read = tcp.read(&mut buf) => match read {
                Ok(0) => break Ok(()),
                Ok(n) => {
                    if let Err(e) = ws.send(Message::binary(buf[..n].to_vec())).await {
                        break Err(format!("viewer socket failed: {e}"));
                    }
                }
                Err(e) => break Err(format!("read from {target} failed: {e}")),
            },
        }
    };

    match outcome {
        Ok(()) => report("closed", target),
        Err(message) => report("error", message),
    }
    let _ = ws.close(None).await;
    let _ = tcp.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;

    /// A TCP server that echoes whatever it receives, standing in for a VNC
    /// server: the bridge must not care what the bytes are.
    async fn echo_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 || socket.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        port
    }

    fn recorder() -> (Reporter, Arc<Mutex<Vec<(&'static str, String)>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        (Arc::new(move |kind, message| sink.lock().unwrap().push((kind, message))), log)
    }

    async fn bridge_to(port: u16) -> (u16, watch::Sender<bool>, Arc<Mutex<Vec<(&'static str, String)>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_port = listener.local_addr().unwrap().port();
        let (cancel, cancelled) = watch::channel(false);
        let (report, log) = recorder();
        tauri::async_runtime::spawn(accept_loop(report, listener, "/token".into(), "127.0.0.1".into(), port, cancelled));
        (ws_port, cancel, log)
    }

    #[tokio::test]
    async fn pipes_binary_frames_to_the_server_and_back() {
        let server = echo_server().await;
        let (ws_port, cancel, log) = bridge_to(server).await;

        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{ws_port}/token")).await.unwrap();
        ws.send(Message::binary(b"RFB 003.008\n".to_vec())).await.unwrap();
        let echoed = ws.next().await.unwrap().unwrap();
        assert_eq!(echoed, Message::binary(b"RFB 003.008\n".to_vec()));

        ws.close(None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let kinds: Vec<&str> = log.lock().unwrap().iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec!["connected", "closed"]);
        let _ = cancel.send(true);
    }

    #[tokio::test]
    async fn rejects_a_websocket_on_the_wrong_path() {
        let server = echo_server().await;
        let (ws_port, cancel, log) = bridge_to(server).await;
        let result = connect_async(format!("ws://127.0.0.1:{ws_port}/guess")).await;
        assert!(result.is_err(), "a foreign path must not reach the server");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(log.lock().unwrap().is_empty(), "nothing was dialled");
        let _ = cancel.send(true);
    }

    #[tokio::test]
    async fn reports_a_refused_server_and_closes_the_socket() {
        // Bind then drop, so the port is free and nothing listens on it.
        let free = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let (ws_port, cancel, log) = bridge_to(free).await;
        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{ws_port}/token")).await.unwrap();
        // The bridge closes the socket: the next frame is Close or end of stream.
        let next = ws.next().await;
        assert!(matches!(next, None | Some(Ok(Message::Close(_)))), "got {next:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let entries = log.lock().unwrap().clone();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "error");
        assert!(entries[0].1.contains("could not connect"), "{}", entries[0].1);
        let _ = cancel.send(true);
    }

    #[tokio::test]
    async fn closing_the_bridge_ends_a_live_session() {
        let server = echo_server().await;
        let (ws_port, cancel, log) = bridge_to(server).await;
        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{ws_port}/token")).await.unwrap();
        ws.send(Message::binary(vec![1, 2, 3])).await.unwrap();
        assert_eq!(ws.next().await.unwrap().unwrap(), Message::binary(vec![1, 2, 3]));

        cancel.send(true).unwrap();
        let next = ws.next().await;
        assert!(matches!(next, None | Some(Ok(Message::Close(_)))), "got {next:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let kinds: Vec<&str> = log.lock().unwrap().iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec!["connected", "closed"]);
    }
}
