//! macOS and Windows run Docker in a VM (Docker Desktop). A container there
//! reaches the host as `host.docker.internal` without any `--add-host`, and
//! Docker Desktop's host-side process relays those connections to the host's
//! own loopback. The collector's 127.0.0.1 listener therefore already takes
//! them, and there is no bridge on the host to bind.
//!
//! Unverified on real hardware, like the rest of the macOS port.

use std::net::IpAddr;

pub fn container_bridge_addrs() -> Vec<IpAddr> {
    Vec::new()
}
