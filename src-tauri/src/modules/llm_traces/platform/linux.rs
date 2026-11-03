//! Native Docker Engine on Linux puts containers on bridge networks: the
//! default `docker0` (172.17.0.1), and one `br-<12 hex>` per user-defined or
//! compose network. A container reaches the host at its network's gateway,
//! which is the host's address on that bridge. `host.docker.internal`, when a
//! container is given it with `--add-host=host.docker.internal:host-gateway`
//! (or compose's `extra_hosts`), resolves to `docker0`'s.
//!
//! Those packets arrive on the bridge, not on loopback, so a listener on
//! 127.0.0.1 refuses them. That is why the collector also binds each bridge
//! address. They are private to this host: nothing on the LAN routes to them.
//! Podman's rootful bridges (`podman0`, `cni-podman<n>`) work the same way.

use std::ffi::CStr;
use std::net::{IpAddr, Ipv4Addr};

/// The host's IPv4 address on every container bridge, sorted. Empty when
/// Docker isn't installed or `getifaddrs` fails; never an error.
pub fn container_bridge_addrs() -> Vec<IpAddr> {
    let mut found = Vec::new();
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
            if i32::from((*ifa.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if !is_container_bridge(&name) {
                continue;
            }
            let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            found.push(IpAddr::V4(Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr))));
        }
        libc::freeifaddrs(list);
    }
    found.sort();
    found.dedup();
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
        // Whatever this machine has, every address found is IPv4 and not loopback.
        for addr in container_bridge_addrs() {
            assert!(addr.is_ipv4());
            assert!(!addr.is_loopback());
        }
    }
}
