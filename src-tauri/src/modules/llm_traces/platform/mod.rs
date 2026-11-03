//! Where Docker containers can reach the collector from.
//!
//! | Function | Linux | macOS, Windows |
//! | --- | --- | --- |
//! | `container_bridge_addrs()` | the host's address on each Docker or Podman bridge (`docker0`, `br-<id>`, `podman0`) | empty: Docker Desktop relays `host.docker.internal` to the host's loopback itself |
//!
//! A backend reads the interfaces and returns what it found; it never binds,
//! retries or decides anything. `server` binds what this returns.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::container_bridge_addrs;

#[cfg(not(target_os = "linux"))]
mod desktop;
#[cfg(not(target_os = "linux"))]
pub use desktop::container_bridge_addrs;
