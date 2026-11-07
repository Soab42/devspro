//! Native Docker Engine on Linux puts containers on bridge networks: the
//! default `docker0` (172.17.0.1), and one `br-<12 hex>` per user-defined or
//! compose network. A container reaches the host at its network's gateway,
//! which is the host's address on that bridge. `host.docker.internal`, when a
//! container is given it with `--add-host=host.docker.internal:host-gateway`
//! (or compose's `extra_hosts`), resolves to `docker0`'s.
//!
//! Those packets arrive on the bridge, not on loopback, so a listener on
//! 127.0.0.1 refuses them. That is why the collector also binds each bridge
//! address. Binding one is not enough to keep it private, though: Linux takes
//! a packet for any of its own addresses on any interface, so a machine on
//! the LAN that routes 172.17.0.0/16 through this one reaches 172.17.0.1 too.
//! That is why each bridge comes with its subnet, and `server` refuses a
//! client on a bridge address unless it is on one of these networks or is
//! this host (see `Networks::admits`).
//! Podman's rootful bridges (`podman0`, `cni-podman<n>`) work the same way.

use super::{Bridge, Networks};
use std::ffi::CStr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The host's IPv4 address on every container bridge, with the bridge's
/// prefix length, and every address of every interface, each sorted. Empty
/// when `getifaddrs` fails; no bridges when Docker isn't installed. Never an
/// error.
pub fn container_networks() -> Networks {
    let mut found = Networks::default();
    // SAFETY: getifaddrs hands back a linked list we only read and then free
    // with freeifaddrs. Every pointer is null-checked before it is read, and
    // nothing borrowed from the list outlives the free.
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut entry = list;
        while !entry.is_null() {
            let ifa = &*entry;
            entry = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
                continue;
            }
            match i32::from((*ifa.ifa_addr).sa_family) {
                libc::AF_INET => {}
                libc::AF_INET6 => {
                    let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    found.own.push(IpAddr::V6(Ipv6Addr::from(addr.sin6_addr.s6_addr)));
                    continue;
                }
                _ => continue,
            }
            let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
            found.own.push(IpAddr::V4(ip));
            let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if !is_container_bridge(&name) {
                continue;
            }
            // No netmask would be odd for a bridge; /32 then admits only the
            // host's own address on it.
            let prefix = if ifa.ifa_netmask.is_null() {
                32
            } else {
                let mask = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                u32::from_be(mask.sin_addr.s_addr).count_ones() as u8
            };
            found.bridges.push(Bridge { addr: ip, prefix });
        }
        libc::freeifaddrs(list);
    }
    found.bridges.sort();
    found.bridges.dedup();
    found.own.sort();
    found.own.dedup();
    found
}

/// `docker0`, Docker's `br-` plus a 12-hex network id, and Podman's bridges.
/// Not every `br-` interface: a hand-made `br-lan` is someone's real network.
fn is_container_bridge(name: &str) -> bool {
    if name == "docker0" || name == "podman0" || name.starts_with("cni-podman") {
        return true;
    }
    name.strip_prefix("br-")
        .is_some_and(|id| id.len() == 12 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_container_bridges_count() {
        assert!(is_container_bridge("docker0"));
        assert!(is_container_bridge("br-083f0bcd8866"));
        assert!(is_container_bridge("podman0"));
        assert!(is_container_bridge("cni-podman1"));
        assert!(!is_container_bridge("br-lan"));
        assert!(!is_container_bridge("br-083f0bcd88"));
        assert!(!is_container_bridge("wlo1"));
        assert!(!is_container_bridge("lo"));
        assert!(!is_container_bridge("virbr0"));
    }

    #[test]
    fn reading_the_interfaces_never_fails() {
        // Whatever this machine has: loopback is among its own addresses, no
        // bridge is loopback, and each bridge's subnet holds the host's own
        // address on it.
        let networks = container_networks();
        assert!(networks.own.contains(&IpAddr::from([127, 0, 0, 1])));
        for bridge in &networks.bridges {
            assert!(!bridge.addr.is_loopback());
            assert!(bridge.prefix <= 32);
            assert!(bridge.contains(bridge.addr.into()));
            assert!(networks.own.contains(&IpAddr::V4(bridge.addr)));
        }
    }
}
