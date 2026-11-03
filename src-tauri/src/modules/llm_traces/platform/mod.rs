//! Where Docker containers can reach the collector from.
//!
//! | Function | Linux | macOS, Windows |
//! | --- | --- | --- |
//! | `container_networks()` | the host's address on each Docker or Podman bridge (`docker0`, `br-<id>`, `podman0`), with the bridge's subnet, and every address the host has | empty: Docker Desktop relays `host.docker.internal` to the host's loopback itself |
//!
//! A backend reads the interfaces and returns what it found; it never binds,
//! retries or decides anything. `server` binds the bridge addresses, and
//! answers a client there only when `Networks::admits` it.

use std::net::{IpAddr, Ipv4Addr};

/// The host's address on one container network, and that network's prefix
/// length: 172.17.0.1/16 for `docker0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bridge {
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

impl Bridge {
    /// Whether `ip` is on this network. An IPv4-mapped IPv6 address counts
    /// as its IPv4 form.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let IpAddr::V4(ip) = ip.to_canonical() else { return false };
        let mask = u32::MAX.checked_shl(32 - u32::from(self.prefix.min(32))).unwrap_or(0);
        u32::from(ip) & mask == u32::from(self.addr) & mask
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Networks {
    /// The container bridges, to bind and to admit clients from.
    pub bridges: Vec<Bridge>,
    /// Every address this host has, on any interface. A process on the host
    /// that connects to a bridge address can arrive from any of them: Docker's
    /// masquerade rule rewrites a bridge source to the LAN address.
    pub own: Vec<IpAddr>,
}

impl Networks {
    /// A container on any of the networks, or this host itself. A machine on
    /// the LAN can't pass for the host: Linux drops a packet that arrives
    /// from outside carrying one of its own addresses as the source.
    pub fn admits(&self, peer: IpAddr) -> bool {
        let peer = peer.to_canonical();
        self.own.contains(&peer) || self.bridges.iter().any(|b| b.contains(peer))
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::container_networks;

#[cfg(not(target_os = "linux"))]
mod desktop;
#[cfg(not(target_os = "linux"))]
pub use desktop::container_networks;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bridge_contains_its_own_subnet_only() {
        let docker0 = Bridge { addr: Ipv4Addr::new(172, 17, 0, 1), prefix: 16 };
        assert!(docker0.contains(IpAddr::from([172, 17, 0, 1])));
        assert!(docker0.contains(IpAddr::from([172, 17, 255, 9])));
        assert!(docker0.contains("::ffff:172.17.0.5".parse().unwrap()));
        assert!(!docker0.contains(IpAddr::from([172, 18, 0, 5])));
        assert!(!docker0.contains(IpAddr::from([192, 168, 1, 20])));
        assert!(!docker0.contains("fe80::1".parse().unwrap()));
        let host_only = Bridge { addr: Ipv4Addr::new(10, 0, 0, 1), prefix: 32 };
        assert!(host_only.contains(IpAddr::from([10, 0, 0, 1])));
        assert!(!host_only.contains(IpAddr::from([10, 0, 0, 2])));
        let everything = Bridge { addr: Ipv4Addr::new(10, 0, 0, 1), prefix: 0 };
        assert!(everything.contains(IpAddr::from([8, 8, 8, 8])));
    }

    #[test]
    fn containers_and_the_host_are_admitted_and_its_neighbours_are_not() {
        let networks = Networks {
            bridges: vec![
                Bridge { addr: Ipv4Addr::new(172, 17, 0, 1), prefix: 16 },
                Bridge { addr: Ipv4Addr::new(172, 18, 0, 1), prefix: 16 },
            ],
            own: vec![IpAddr::from([127, 0, 0, 1]), IpAddr::from([192, 168, 0, 104]), "::1".parse().unwrap()],
        };
        assert!(networks.admits(IpAddr::from([172, 18, 0, 5])));
        assert!(networks.admits(IpAddr::from([192, 168, 0, 104])));
        assert!(networks.admits("::ffff:192.168.0.104".parse().unwrap()));
        assert!(networks.admits("::1".parse().unwrap()));
        assert!(!networks.admits(IpAddr::from([192, 168, 0, 20])));
        assert!(!networks.admits(IpAddr::from([172, 21, 0, 5])));
    }
}
