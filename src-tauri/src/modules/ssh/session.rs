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
use std::path::{Path, PathBuf};
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

/// The key files `ssh` looks for when none is configured, less the
/// security-key and XMSS kinds libssh2 can't use.
const DEFAULT_IDENTITIES: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// The public half of a private key file, read from the `.pub` beside it, in
/// the form an agent lists its keys — how an agent key is matched to a file.
fn public_key_blob(private: &Path) -> Option<Vec<u8>> {
    let mut path = private.as_os_str().to_owned();
    path.push(".pub");
    let line = std::fs::read_to_string(path).ok()?;
    STANDARD.decode(line.split_whitespace().nth(1)?).ok()
}

/// Offers every key in the agent; `Session::userauth_agent` only ever tries
/// the first. Keys in `preferred` go first, as `ssh` puts the keys it also has
/// files for first: in agent order, a few unrelated keys can use up the
/// server's MaxAuthTries before the right one. Returns the keys offered.
fn authenticate_agent(sess: &Session, user: &str, preferred: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let Ok(mut agent) = sess.agent() else { return Vec::new() };
    if agent.connect().is_err() || agent.list_identities().is_err() {
        return Vec::new();
    }
    let mut keys = agent.identities().unwrap_or_default();
    keys.sort_by_key(|key| {
        preferred
            .iter()
            .position(|blob| blob.as_slice() == key.blob())
            .unwrap_or(usize::MAX)
    });
    let mut offered = Vec::new();
    for key in &keys {
        offered.push(key.blob().to_vec());
        if agent.userauth(user, key).is_ok() && sess.authenticated() {
            break;
        }
    }
    let _ = agent.disconnect();
    offered
}

/// libssh2's code for a key file it couldn't load: unreadable, an unknown
/// format, or — the usual case — encrypted and the passphrase missing or wrong.
/// A key the server turned down comes back with a different code.
const LIBSSH2_ERROR_FILE: i32 = -16;

fn could_not_load(e: &ssh2::Error) -> bool {
    matches!(e.code(), ssh2::ErrorCode::Session(LIBSSH2_ERROR_FILE))
}

/// Private keys in `~/.ssh` (those with a `.pub` beside them) that none of
/// `tried` covers, by file name: what to point the user at when nothing
/// worked, since `ssh` never offers a key by itself unless it has a default
/// name.
fn untried_keys(tried: &[PathBuf], offered: &[Vec<u8>]) -> Vec<String> {
    let Ok(dir) = super::ssh_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let tried: Vec<String> = tried.iter().map(|p| super::config::normalize(p)).collect();
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("pub"))
        .map(|p| p.with_extension(""))
        .filter(|p| p.is_file() && !tried.contains(&super::config::normalize(p)))
        .filter(|p| public_key_blob(p).map_or(true, |blob| !offered.contains(&blob)))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// What `ssh` tries, in its order, so that a host the terminal (the real
/// `ssh`) logs in to connects here too: every agent key, then the host's key
/// file, then the `IdentityFile`s `~/.ssh/config` gives this host, then the
/// default `~/.ssh` keys, then the password. A key file that isn't there is
/// noted and skipped, as `ssh` does with a missing `-i`, rather than ending
/// the attempt. The agent goes first because a key loaded into it shouldn't
/// need its passphrase typed again.
fn authenticate(sess: &Session, host: &SshHost, creds: &Credentials) -> Result<(), String> {
    let user = host.username.as_str();
    let mut failures: Vec<String> = Vec::new();

    let key_file = host
        .key_path
        .as_deref()
        .filter(|k| host.auth == "key" && !k.trim().is_empty())
        .map(PathBuf::from);
    let same = |a: &Path, b: &Path| super::config::normalize(a) == super::config::normalize(b);
    let configured: Vec<PathBuf> = super::config::identity_files(&super::config::Target {
        host: &host.host,
        port: host.port,
        user,
    })
    .into_iter()
    .filter(|p| !key_file.as_deref().is_some_and(|k| same(k, p)))
    .collect();
    let defaults: Vec<PathBuf> = super::ssh_dir()
        .map(|dir| DEFAULT_IDENTITIES.iter().map(|name| dir.join(name)).collect())
        .unwrap_or_default();
    let preferred: Vec<Vec<u8>> = key_file
        .iter()
        .chain(&configured)
        .chain(&defaults)
        .filter_map(|path| public_key_blob(path))
        .collect();

    let offered = authenticate_agent(sess, user, &preferred);
    if sess.authenticated() {
        return Ok(());
    }
    if !offered.is_empty() {
        failures.push("ssh-agent has no key the server accepts".to_string());
    } else if host.auth == "agent" {
        failures.push("ssh-agent has no keys loaded".to_string());
    }
    // A file whose key the agent already offered would only be refused again.
    let already_offered = |path: &Path| public_key_blob(path).is_some_and(|blob| offered.contains(&blob));

    if let Some(path) = &key_file {
        if !path.is_file() {
            failures.push(format!("private key not found: {}", path.display()));
        } else if !already_offered(path) {
            match sess.userauth_pubkey_file(user, None, path, creds.passphrase.as_deref()) {
                Ok(()) if sess.authenticated() => return Ok(()),
                Ok(()) => failures.push("key was not accepted".to_string()),
                Err(e) => failures.push(format!("key auth failed: {}", e.message())),
            }
        }
    }

    // The config's keys, which `ssh` would use for this host too. A typed
    // passphrase is for them when the host has no key of its own; an
    // encrypted one that can't be opened is reported as "key auth failed",
    // which is what makes the UI ask for the passphrase.
    let config_passphrase = if key_file.is_none() { creds.passphrase.as_deref() } else { None };
    for path in &configured {
        if !path.is_file() {
            failures.push(format!("~/.ssh/config names a key that isn't there: {}", path.display()));
            continue;
        }
        if already_offered(path) {
            continue;
        }
        match sess.userauth_pubkey_file(user, None, path, config_passphrase) {
            Ok(()) if sess.authenticated() => return Ok(()),
            Err(e) if could_not_load(&e) => {
                failures.push(format!("key auth failed for {} (from ~/.ssh/config): {}", path.display(), e.message()))
            }
            _ => failures.push(format!("{} (from ~/.ssh/config) was not accepted", path.display())),
        }
    }

    // No passphrase: one the user typed is for the host's own key. An
    // encrypted default key then fails to load here, before anything is sent,
    // so it costs no attempt against the server.
    for path in &defaults {
        if key_file.as_ref() == Some(path) || configured.iter().any(|c| same(c, path)) || !path.is_file() || already_offered(path) {
            continue;
        }
        if sess.userauth_pubkey_file(user, None, path, None).is_ok() && sess.authenticated() {
            return Ok(());
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

    if host.auth != "password" {
        let tried: Vec<PathBuf> = key_file.iter().chain(&configured).chain(&defaults).cloned().collect();
        let untried = untried_keys(&tried, &offered);
        if !untried.is_empty() {
            failures.push(format!("{UNTRIED_KEYS_PREFIX}{}", untried.join(", ")));
        }
    }

    Err(format!(
        "authentication failed for {user}@{}: {}",
        host.host,
        failures.join("; ")
    ))
}

/// Starts the part of an auth failure that lists `~/.ssh` keys nothing
/// offered. The UI matches on it, so it is part of the command contract.
pub const UNTRIED_KEYS_PREFIX: &str = "not tried (not a default name, not in ~/.ssh/config): ";

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
    fn public_key_blob_reads_the_pub_file_beside_the_key() {
        let dir = std::env::temp_dir().join(format!("devspro-pubkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("id_test");
        std::fs::write(dir.join("id_test.pub"), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 me@box\n").unwrap();
        assert_eq!(public_key_blob(&key).as_deref(), Some(&b"\0\0\0\x0bssh-ed25519"[..]));
        assert_eq!(public_key_blob(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn known_hosts_name_brackets_non_default_ports_only() {
        assert_eq!(known_hosts_name("example.com", 22), "example.com");
        assert_eq!(known_hosts_name("example.com", 2222), "[example.com]:2222");
    }
}
