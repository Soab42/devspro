//! Remote desktops: saved VNC and RDP connections, a VNC viewer that runs
//! inside the app, and a launcher for the system's RDP client.
//!
//! Two transports, deliberately different:
//!   - **VNC** is viewed in-app. The frontend runs noVNC in the webview, which
//!     only speaks WebSocket, so `vnc.rs` opens a loopback WebSocket listener
//!     for each session and pipes it byte-for-byte to the VNC server's TCP
//!     port. The RFB protocol itself — encodings, auth, input — is entirely
//!     noVNC's business; Rust never inspects a frame.
//!   - **RDP** launches an installed client (FreeRDP, Remmina, rdesktop; on
//!     Windows, Remote Desktop Connection or FreeRDP) in its own window
//!     (`rdp.rs`, `mstsc.rs`). An RDP session needs TLS, CredSSP and a
//!     bitmap codec stack; wrapping the system client gives all of it, with
//!     the client's own dialogs for certificates and credentials, for the
//!     cost of building an argument list.
//!
//! Passwords are never written to the database, matching the SSH module:
//! they are asked for at connect time and travel straight to the session.

pub mod connections;
#[cfg(windows)]
mod mstsc;
pub mod rdp;
pub mod vnc;

/// Live VNC bridges. Each is a loopback listener plus the tasks piping its
/// accepted sockets; none of it survives a restart, so nothing is persisted.
#[derive(Default)]
pub struct RemoteDesktopState {
    pub vnc: vnc::BridgeMap,
}
