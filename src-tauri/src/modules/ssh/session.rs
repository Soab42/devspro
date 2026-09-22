//! Opening an authenticated `ssh2` session to a saved host — shared by the
//! SFTP browser and by "install key on host".
//!
//! Host-key trust is handled the way OpenSSH does it, against the same
//! `~/.ssh/known_hosts`, so a host trusted from the terminal (which runs the
//! real `ssh`) is trusted here too and vice versa. An unknown host is not
//! auto-accepted: the error carries the fingerprint (`UNKNOWN_HOST:<fp>`) so
//! the UI can show a trust dialog, and `trust_host` writes the entry once the
//! user says yes. A *changed* key is a hard error — that's the case the
//! mechanism exists for.

use super::hosts::SshHost;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ssh2::{CheckResult, HashType, KnownHostFileKind, Session};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// Prefix on the error string when the host key is not in known_hosts. The UI
/// matches on it, so it is part of the command contract.
pub const UNKNOWN_HOST_PREFIX: &str = "UNKNOWN_HOST:";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// libssh2 operations block; without a timeout a dead link would hang a
/// command (and the UI waiting on it) forever.
const IO_TIMEOUT_MS: u32 = 30_000;

/// What the user has typed for this one connection attempt. Never stored.
#[derive(Default, Clone)]
pub struct Credentials {
    pub password: Option<String>,
    pub passphrase: Option<String>,
}

/// OpenSSH's presentation of a SHA-256 host-key hash: base64 without padding,
/// so what we show matches what `ssh` prints in its own trust prompt and the
/// user can compare the two directly.
pub fn fingerprint_sha256(hash: &[u8]) -> String {
    let encoded = STANDARD.encode(hash);
    format!("SHA256:{}", encoded.trim_end_matches('='))
}

/// The host name as known_hosts spells it: bare for port 22, `[host]:port`
/// otherwise. libssh2's `check_port` understands both, but `add` takes the
/// literal string, so writing it in the wrong form would produce an entry that
/// never matches.
pub fn known_hosts_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn known_hosts_path() -> Result<std::path::PathBuf, String> {
    Ok(super::ssh_dir()?.join("known_hosts"))
}

/// TCP + SSH handshake, no auth, no host-key check. Everything else builds on
/// this so that the trust flow and the real connection go through one path.
fn handshake(host: &SshHost) -> Result<Session, String> {
    let addr = (host.host.as_str(), host.port)
        .to_socket_addrs()
        .map_err(|e| format!("could not resolve {}: {e}", host.host))?
        .next()
        .ok_or_else(|| format!("could not resolve {}", host.host))?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("could not connect to {}:{}: {e}", host.host, host.port))?;
    let _ = tcp.set_read_timeout(Some(CONNECT_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(CONNECT_TIMEOUT));
    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_tcp_stream(tcp);
    sess.set_timeout(IO_TIMEOUT_MS);
    sess.handshake()
        .map_err(|e| format!("SSH handshake with {} failed: {}", host.host, e.message()))?;
    Ok(sess)
}

fn verify_host_key(sess: &Session, host: &SshHost) -> Result<(), String> {
    let (key, _) = sess.host_key().ok_or("server sent no host key")?;
    let fingerprint = sess
        .host_key_hash(HashType::Sha256)
        .map(fingerprint_sha256)
        .unwrap_or_else(|| "(unavailable)".to_string());

    let mut known = sess.known_hosts().map_err(|e| e.to_string())?;
    let path = known_hosts_path()?;
    if path.is_file() {
        known
            .read_file(&path, KnownHostFileKind::OpenSSH)
            .map_err(|e| format!("could not read {}: {}", path.display(), e.message()))?;
    }
    match known.check_port(&host.host, host.port, key) {
        CheckResult::Match => Ok(()),
        CheckResult::NotFound => Err(format!("{UNKNOWN_HOST_PREFIX}{fingerprint}")),
        CheckResult::Mismatch => Err(format!(
            "host key for {} has CHANGED (now {fingerprint}). This could be a man-in-the-middle attack; \
             if the server was legitimately reinstalled, remove its line from ~/.ssh/known_hosts.",
            host.host
        )),
        CheckResult::Failure => Err("host key check failed".to_string()),
    }
}

/// Records the host's current key in known_hosts. Returns the fingerprint that
/// was trusted so the UI can confirm it.
pub fn trust_host(host: &SshHost) -> Result<String, String> {
    let sess = handshake(host)?;
    let (key, key_type) = sess.host_key().ok_or("server sent no host key")?;
    let fingerprint = sess
        .host_key_hash(HashType::Sha256)
        .map(fingerprint_sha256)
        .unwrap_or_default();

    let dir = super::ensure_ssh_dir()?;
    let path = dir.join("known_hosts");
    let mut known = sess.known_hosts().map_err(|e| e.to_string())?;
    if path.is_file() {
        known
            .read_file(&path, KnownHostFileKind::OpenSSH)
            .map_err(|e| format!("could not read {}: {}", path.display(), e.message()))?;
    }
    known
        .add(&known_hosts_name(&host.host, host.port), key, "", key_type.into())
        .map_err(|e| e.message().to_string())?;
    known
        .write_file(&path, KnownHostFileKind::OpenSSH)
        .map_err(|e| format!("could not write {}: {}", path.display(), e.message()))?;
    Ok(fingerprint)
}

/// Agent, then the configured key file, then password — the same order `ssh`
/// tries them. The agent is always attempted first because it is free when it
/// works and fails instantly when it doesn't, and a key that is loaded into the
/// agent shouldn't need its passphrase typed again.
fn authenticate(sess: &Session, host: &SshHost, creds: &Credentials) -> Result<(), String> {
    let user = host.username.as_str();
    let mut failures: Vec<String> = Vec::new();

    if sess.userauth_agent(user).is_ok() && sess.authenticated() {
        return Ok(());
    }
    if host.auth == "agent" {
        failures.push("ssh-agent has no key the server accepts".to_string());
    }

    if let Some(key_path) = host.key_path.as_deref().filter(|_| host.auth == "key") {
        let path = Path::new(key_path);
        if !path.is_file() {
            return Err(format!("private key not found: {key_path}"));
        }
        match sess.userauth_pubkey_file(user, None, path, creds.passphrase.as_deref()) {
            Ok(()) if sess.authenticated() => return Ok(()),
            Ok(()) => failures.push("key was not accepted".to_string()),
            Err(e) => failures.push(format!("key auth failed: {}", e.message())),
        }
    }

    if host.auth == "password" {
        match creds.password.as_deref() {
            None => return Err("PASSWORD_REQUIRED".to_string()),
            Some(pw) => match sess.userauth_password(user, pw) {
                Ok(()) if sess.authenticated() => return Ok(()),
                _ => failures.push("password rejected".to_string()),
            },
        }
    }

    Err(format!(
        "authentication failed for {user}@{}: {}",
        host.host,
        failures.join("; ")
    ))
}

/// Full connection: handshake, host-key verification, authentication.
pub fn connect(host: &SshHost, creds: &Credentials) -> Result<Session, String> {
    let sess = handshake(host)?;
    verify_host_key(&sess, host)?;
    authenticate(&sess, host, creds)?;
    Ok(sess)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_matches_openssh_presentation() {
        // OpenSSH strips the base64 padding; a 32-byte SHA-256 always ends in "=".
        let hash = [0u8; 32];
        let fp = fingerprint_sha256(&hash);
        assert!(fp.starts_with("SHA256:"));
        assert!(!fp.ends_with('='));
        assert_eq!(fp, "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn known_hosts_name_brackets_non_default_ports_only() {
        assert_eq!(known_hosts_name("example.com", 22), "example.com");
        assert_eq!(known_hosts_name("example.com", 2222), "[example.com]:2222");
    }
}
