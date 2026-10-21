//! Ngrok-style public tunnel for a locally-running nginx/SSL site — spawns
//! `cloudflared`'s anonymous "quick tunnel" pointed at the site's own
//! `https://<hostname>` endpoint and parses the assigned
//! `https://<random>.trycloudflare.com` URL out of its (noisy, ANSI-colored,
//! box-drawn) log output.
//!
//! `cloudflared` was picked over `ngrok`: modern `ngrok http` refuses to run at
//! all until the user signs up and adds a free-tier auth token, which is a bad
//! first-run experience for a feature that's supposed to be one click.
//! `cloudflared tunnel --url ...` needs nothing but the binary — no account, no
//! token. Swapping backends later only means changing `spawn_quick_tunnel` and
//! `parse_quick_tunnel_url` (kept as the one small, swappable seam), not a
//! whole provider abstraction.
//!
//! Every site — `Backend::Port` or `Backend::Laravel` — is actually served by
//! nginx on 443 with SNI/Host routing per hostname (see
//! `nginx_ssl::nginx::render`); the backend port itself is only reachable
//! through that proxy, not directly from the outside. So the tunnel always
//! targets `https://<hostname>`, never a raw backend port, and nginx's
//! locally-trusted (but not CA-known-to-cloudflared) cert is why
//! `--no-tls-verify` is required.
//!
//! In-memory only, like `docker`/`system`: a tunnel is a live child process,
//! not something to restore across restarts. Killed via `RunEvent::ExitRequested`
//! in `lib.rs` so it never outlives the app.

use crate::modules::nginx_ssl::sites;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Exposed so the Apps catalog can assert it installs cloudflared under exactly
/// this name — install it as anything else and the Tunnel button stays disabled
/// after an apparently successful install.
pub(crate) const BINARY: &str = "cloudflared";
const URL_SUFFIX: &str = ".trycloudflare.com";
const RECENT_LOG_LINES: usize = 20;

#[derive(Debug, Default)]
struct RuntimeInfo {
    public_url: Option<String>,
    recent_lines: VecDeque<String>,
}

struct ActiveTunnel {
    child: Child,
    hostname: String,
    info: Arc<Mutex<RuntimeInfo>>,
}

#[derive(Default)]
pub struct TunnelState(Mutex<HashMap<String, ActiveTunnel>>);

#[derive(Debug, Serialize, Clone)]
pub struct TunnelDetection {
    pub found: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct TunnelStatus {
    pub site_id: String,
    pub hostname: String,
    pub public_url: Option<String>,
    pub pid: u32,
    /// True once the cloudflared process has exited on its own (crashed,
    /// killed externally, ...) — the frontend treats this the same as "not
    /// running" and shows `last_log` instead of a stop button.
    pub exited: bool,
    pub last_log: Option<String>,
}

/// Strips ANSI CSI escape sequences (`ESC '[' ... <letter>`), which is all
/// that cloudflared's colorized log level tags (`\x1b[32mINF\x1b[0m`) use.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            let mut lookahead = chars.clone();
            if lookahead.next() == Some('[') {
                chars.next();
                for nc in chars.by_ref() {
                    if nc.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn is_url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/' | ':' | '?' | '=' | '&' | '%' | '#')
}

/// Pulls the `https://<random-words>.trycloudflare.com` URL out of one line
/// (or a whole multi-line chunk) of cloudflared's quick-tunnel output.
/// Cloudflared wraps it in a decorative ASCII box, prefixes every line with a
/// timestamp and a colorized `INF` level tag, and pads the URL with spaces —
/// none of which matters here, since this only looks for a run of URL-safe
/// characters starting at `https://` and checks whether it's the tunnel's own
/// `trycloudflare.com` domain (other real `https://` links appear in
/// cloudflared's startup disclaimer text and must not match).
fn parse_quick_tunnel_url(text: &str) -> Option<String> {
    let stripped = strip_ansi(text);
    let mut search_from = 0;
    while let Some(rel) = stripped[search_from..].find("https://") {
        let start = search_from + rel;
        let end = stripped[start..]
            .find(|c: char| !is_url_char(c))
            .map(|offset| start + offset)
            .unwrap_or(stripped.len());
        let candidate = &stripped[start..end];
        if candidate.contains(URL_SUFFIX) {
            return Some(candidate.to_string());
        }
        if end <= start {
            break;
        }
        search_from = end;
    }
    None
}

fn require_binary() -> Result<(), String> {
    if crate::modules::tools::is_available(BINARY) {
        Ok(())
    } else {
        Err(format!(
            "{BINARY} was not found on this machine — install it from \
             https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ first"
        ))
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(reader: R, info: Arc<Mutex<RuntimeInfo>>) {
    std::thread::spawn(move || {
        let buffered = std::io::BufReader::new(reader);
        for line in buffered.lines() {
            let Ok(line) = line else { break };
            let mut guard = info.lock().unwrap();
            if guard.public_url.is_none() {
                if let Some(url) = parse_quick_tunnel_url(&line) {
                    guard.public_url = Some(url);
                }
            }
            guard.recent_lines.push_back(line);
            if guard.recent_lines.len() > RECENT_LOG_LINES {
                guard.recent_lines.pop_front();
            }
        }
    });
}

fn spawn_quick_tunnel(target_url: &str) -> Result<(Child, Arc<Mutex<RuntimeInfo>>), String> {
    let mut child = Command::new(BINARY)
        .args(["tunnel", "--no-tls-verify", "--url", target_url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {BINARY}: {e}"))?;

    let info = Arc::new(Mutex::new(RuntimeInfo::default()));
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, info.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, info.clone());
    }
    Ok((child, info))
}

#[tauri::command(async)]
pub fn tunnel_detect() -> TunnelDetection {
    TunnelDetection { found: crate::modules::tools::is_available(BINARY) }
}

#[tauri::command(async)]
pub fn tunnel_start(
    db: tauri::State<crate::db::Db>,
    state: tauri::State<TunnelState>,
    site_id: String,
) -> Result<TunnelStatus, String> {
    require_binary()?;

    let mut tunnels = state.0.lock().map_err(|e| e.to_string())?;
    if tunnels.contains_key(&site_id) {
        return Err("a tunnel is already running for this site".to_string());
    }

    let site = sites::find_by_id(&db, &site_id)?;
    if !site.enabled {
        return Err("enable this site before tunneling it — a disabled site isn't served by nginx".to_string());
    }

    let target = format!("https://{}", site.hostname);
    let (child, info) = spawn_quick_tunnel(&target)?;
    let pid = child.id();

    tunnels.insert(
        site_id.clone(),
        ActiveTunnel {
            child,
            hostname: site.hostname.clone(),
            info,
        },
    );

    Ok(TunnelStatus {
        site_id,
        hostname: site.hostname,
        public_url: None,
        pid,
        exited: false,
        last_log: None,
    })
}

#[tauri::command(async)]
pub fn tunnel_stop(state: tauri::State<TunnelState>, site_id: String) -> Result<(), String> {
    let mut tunnels = state.0.lock().map_err(|e| e.to_string())?;
    let mut tunnel = tunnels
        .remove(&site_id)
        .ok_or_else(|| "no tunnel is running for this site".to_string())?;
    let _ = tunnel.child.kill();
    let _ = tunnel.child.wait();
    Ok(())
}

#[tauri::command(async)]
pub fn tunnel_list(state: tauri::State<TunnelState>) -> Result<Vec<TunnelStatus>, String> {
    let mut tunnels = state.0.lock().map_err(|e| e.to_string())?;
    let mut statuses: Vec<TunnelStatus> = Vec::with_capacity(tunnels.len());
    let mut exited_ids: Vec<String> = Vec::new();
    for (site_id, tunnel) in tunnels.iter_mut() {
        let exited = matches!(tunnel.child.try_wait(), Ok(Some(_)));
        if exited {
            exited_ids.push(site_id.clone());
        }
        let guard = tunnel.info.lock().unwrap();
        let public_url = guard.public_url.clone();
        let last_log = guard.recent_lines.back().cloned();
        statuses.push(TunnelStatus {
            site_id: site_id.clone(),
            hostname: tunnel.hostname.clone(),
            public_url,
            pid: tunnel.child.id(),
            exited,
            last_log,
        });
    }
    // Reported once above, then dropped — otherwise a crashed cloudflared
    // process would permanently occupy this site's slot in the map and
    // `tunnel_start` would keep refusing to start a new one for it.
    for id in exited_ids {
        tunnels.remove(&id);
    }
    Ok(statuses)
}

/// Called from `lib.rs`'s `RunEvent::ExitRequested` handler so a tunnel never
/// outlives the app it was started from.
pub fn kill_all(state: tauri::State<TunnelState>) {
    if let Ok(mut tunnels) = state.0.lock() {
        for (_, mut tunnel) in tunnels.drain() {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_clean_url_with_no_surrounding_noise() {
        assert_eq!(
            parse_quick_tunnel_url("https://wise-juvenile-pink-recovered.trycloudflare.com"),
            Some("https://wise-juvenile-pink-recovered.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn parses_the_real_boxed_log_line_with_ansi_color_codes() {
        // What cloudflared actually prints, roughly verbatim: timestamp, a
        // colorized "INF" tag, then the URL padded inside a `|...|` box.
        let line = "2024-05-01T12:00:00Z \u{1b}[32mINF\u{1b}[0m |  https://particular-example-words-here.trycloudflare.com                                   |";
        assert_eq!(
            parse_quick_tunnel_url(line),
            Some("https://particular-example-words-here.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn ignores_unrelated_https_links_in_the_startup_disclaimer() {
        let line = "2024-05-01T12:00:00Z \u{1b}[32mINF\u{1b}[0m If you intend to use Tunnels in production you should use a pre-created named tunnel by following the documentation at https://developers.cloudflare.com/cloudflare-one/connections/connect-apps";
        assert_eq!(parse_quick_tunnel_url(line), None);
    }

    #[test]
    fn finds_the_real_url_in_a_full_multi_line_startup_block() {
        let block = "\u{1b}[32mINF\u{1b}[0m Thank you for trying Cloudflare Tunnel. Doing so, without a Cloudflare account, is a quick way to experiment and try it out. However, be aware that these account-less Tunnels have no uptime guarantee. If you intend to use Tunnels in production you should use a pre-created named tunnel by following the documentation at https://developers.cloudflare.com/cloudflare-one/connections/connect-apps\n\u{1b}[32mINF\u{1b}[0m Requesting new quick Tunnel on trycloudflare.com...\n\u{1b}[32mINF\u{1b}[0m +--------------------------------------------------------------------------------------------+\n\u{1b}[32mINF\u{1b}[0m |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |\n\u{1b}[32mINF\u{1b}[0m |  https://random-words-here.trycloudflare.com                                               |\n\u{1b}[32mINF\u{1b}[0m +--------------------------------------------------------------------------------------------+";
        assert_eq!(
            parse_quick_tunnel_url(block),
            Some("https://random-words-here.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_url_is_present_yet() {
        assert_eq!(
            parse_quick_tunnel_url("2024-05-01T12:00:00Z \u{1b}[32mINF\u{1b}[0m Starting tunnel..."),
            None
        );
    }

    #[test]
    fn strip_ansi_removes_csi_color_sequences_only() {
        assert_eq!(strip_ansi("\u{1b}[32mINF\u{1b}[0m hello"), "INF hello");
        assert_eq!(strip_ansi("no escapes here"), "no escapes here");
    }
}
