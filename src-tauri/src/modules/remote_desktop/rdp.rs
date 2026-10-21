//! RDP: launch the system's client in its own window.
//!
//! An RDP session needs TLS, CredSSP/NLA and a stack of bitmap codecs; the
//! installed client already has all of it, plus its own dialogs for
//! certificates. So this file finds whichever client is on `PATH`, turns a
//! saved connection into that client's argument list, and reports how the
//! process ended so the UI can show "wrong password" rather than nothing.
//!
//! The password, when the user gives one, goes on the command line: that's
//! the only way every one of these clients accepts it without a terminal, and
//! it is visible to other local users in `ps` for as long as the client runs.
//! The prompt says so. Leaving the field empty lets the client ask (Remmina
//! has a dialog; FreeRDP without a terminal will simply fail to authenticate).

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

use super::connections::RemoteDesktop;

pub const EXIT_EVENT: &str = "remote-desktop:rdp-exit";

/// Known clients in order of preference. FreeRDP 3 first: it is the current
/// series and the one modern distributions package.
const CANDIDATES: [(&str, &str); 5] = [
    ("xfreerdp3", "FreeRDP 3"),
    ("xfreerdp", "FreeRDP 2"),
    ("wlfreerdp", "FreeRDP (Wayland)"),
    ("remmina", "Remmina"),
    ("rdesktop", "rdesktop"),
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RdpClient {
    /// The executable name; what the launch command is given back.
    pub id: String,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RdpLaunch {
    pub pid: u32,
    pub client: String,
}

#[derive(Debug, Clone, Serialize)]
struct ExitEvent {
    pid: u32,
    client: String,
    /// None when the client was killed by a signal.
    code: Option<i32>,
    /// The last lines the client wrote to stderr — FreeRDP puts the reason
    /// for a failed connection there.
    output: String,
}

fn is_executable(path: &Path) -> bool {
    crate::modules::fs_compat::is_executable(path)
}

fn find_on_path(name: &str, path_var: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| is_executable(candidate))
}

pub fn detect(path_var: &str) -> Vec<RdpClient> {
    CANDIDATES
        .iter()
        .filter_map(|(id, label)| {
            find_on_path(id, path_var).map(|path| RdpClient {
                id: id.to_string(),
                label: label.to_string(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

/// The argument list for one client. Pure, so the FreeRDP 2/3 flag
/// differences — the ones that break a launch silently — are pinned by tests.
pub fn arguments(client: &str, desktop: &RemoteDesktop, password: Option<&str>) -> Vec<String> {
    let target = format!("{}:{}", desktop.host, desktop.port);
    let password = password.filter(|p| !p.is_empty());
    match client {
        "xfreerdp3" | "xfreerdp" | "wlfreerdp" => {
            let mut args = vec![format!("/v:{target}"), format!("/u:{}", desktop.username)];
            if !desktop.domain.is_empty() {
                args.push(format!("/d:{}", desktop.domain));
            }
            if let Some(p) = password {
                args.push(format!("/p:{p}"));
            }
            args.push(format!("/t:{}", desktop.name));
            // Trust the certificate on first use, the way ssh does: without a
            // terminal the client cannot ask, so the alternative is a launch
            // that dies before the window appears.
            args.push(if client == "xfreerdp" { "/cert-tofu".to_string() } else { "/cert:tofu".to_string() });
            args.push("+clipboard".to_string());
            if desktop.fullscreen {
                args.push("/f".to_string());
            } else {
                args.push("/dynamic-resolution".to_string());
            }
            args
        }
        "remmina" => {
            let user = if desktop.domain.is_empty() {
                desktop.username.clone()
            } else {
                format!("{}\\{}", desktop.domain, desktop.username)
            };
            let auth = match password {
                Some(p) => format!("{user}:{p}"),
                None => user,
            };
            vec!["-c".to_string(), format!("rdp://{auth}@{target}")]
        }
        "rdesktop" => {
            let mut args = vec!["-u".to_string(), desktop.username.clone()];
            if !desktop.domain.is_empty() {
                args.push("-d".to_string());
                args.push(desktop.domain.clone());
            }
            if let Some(p) = password {
                args.push("-p".to_string());
                args.push(p.to_string());
            }
            args.push("-T".to_string());
            args.push(desktop.name.clone());
            if desktop.fullscreen {
                args.push("-f".to_string());
            } else {
                args.push("-g".to_string());
                args.push("workarea".to_string());
            }
            args.push(target);
            args
        }
        other => vec![other.to_string()],
    }
}

#[tauri::command(async)]
pub fn remote_desktop_rdp_clients() -> Vec<RdpClient> {
    detect(&std::env::var("PATH").unwrap_or_default())
}

#[tauri::command(async)]
pub fn remote_desktop_rdp_launch(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    client: String,
    password: Option<String>,
) -> Result<RdpLaunch, String> {
    let desktop = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::connections::find(&conn, &id)?
    };
    if desktop.protocol != "rdp" {
        return Err("this connection is not an RDP connection".to_string());
    }
    let installed = detect(&std::env::var("PATH").unwrap_or_default());
    let Some(found) = installed.into_iter().find(|c| c.id == client) else {
        return Err(format!("{client} is not installed (or not on PATH)"));
    };

    let args = arguments(&found.id, &desktop, password.as_deref());
    let mut child = Command::new(&found.path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", found.label))?;
    let pid = child.id();

    // Reap the client in the background and tell the UI how it ended. The
    // client is a peer window, not a child we manage: quitting the app
    // leaves it running.
    let label = found.label.clone();
    let mut stderr = child.stderr.take();
    std::thread::spawn(move || {
        let mut output = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut output);
        }
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = app.emit(EXIT_EVENT, ExitEvent { pid, client: label, code, output: tail(&output, 12) });
    });

    Ok(RdpLaunch { pid, client: found.label })
}

/// The last `lines` non-empty lines, so a wall of FreeRDP debug output ends
/// with the line that matters.
fn tail(output: &str, lines: usize) -> String {
    let kept: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = kept.len().saturating_sub(lines);
    kept[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop(fullscreen: bool, domain: &str) -> RemoteDesktop {
        RemoteDesktop {
            id: "d".into(),
            name: "Office".into(),
            protocol: "rdp".into(),
            host: "pc.local".into(),
            port: 3389,
            username: "alice".into(),
            domain: domain.into(),
            view_only: false,
            scale: false,
            fullscreen,
            created_at: "now".into(),
        }
    }

    #[test]
    fn freerdp3_and_freerdp2_differ_only_in_the_tofu_flag() {
        let three = arguments("xfreerdp3", &desktop(false, "CORP"), Some("pw"));
        assert_eq!(
            three,
            vec![
                "/v:pc.local:3389",
                "/u:alice",
                "/d:CORP",
                "/p:pw",
                "/t:Office",
                "/cert:tofu",
                "+clipboard",
                "/dynamic-resolution"
            ]
        );
        let two = arguments("xfreerdp", &desktop(false, "CORP"), Some("pw"));
        assert!(two.contains(&"/cert-tofu".to_string()));
        assert!(!two.contains(&"/cert:tofu".to_string()));
    }

    #[test]
    fn freerdp_omits_empty_password_and_domain_and_honours_fullscreen() {
        let args = arguments("xfreerdp3", &desktop(true, ""), Some(""));
        assert!(!args.iter().any(|a| a.starts_with("/p:") || a.starts_with("/d:")));
        assert!(args.contains(&"/f".to_string()));
        assert!(!args.contains(&"/dynamic-resolution".to_string()));
    }

    #[test]
    fn remmina_gets_a_url_with_domain_and_password() {
        assert_eq!(
            arguments("remmina", &desktop(false, "CORP"), Some("pw")),
            vec!["-c", "rdp://CORP\\alice:pw@pc.local:3389"]
        );
        assert_eq!(
            arguments("remmina", &desktop(false, ""), None),
            vec!["-c", "rdp://alice@pc.local:3389"]
        );
    }

    #[test]
    fn rdesktop_puts_the_target_last() {
        let args = arguments("rdesktop", &desktop(false, ""), Some("pw"));
        assert_eq!(args.last().map(String::as_str), Some("pc.local:3389"));
        assert!(args.windows(2).any(|w| w == ["-p", "pw"]));
        assert!(args.windows(2).any(|w| w == ["-g", "workarea"]));
    }

    #[test]
    fn detect_finds_executables_on_the_given_path_in_preference_order() {
        let dir = std::env::temp_dir().join(format!("devspro-rdp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["remmina", "xfreerdp3"] {
            let p = dir.join(name);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // A non-executable file must not count.
        std::fs::write(dir.join("rdesktop"), "").unwrap();

        let found = detect(&format!("/nonexistent:{}", dir.display()));
        assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["xfreerdp3", "remmina"]);
        assert_eq!(found[0].label, "FreeRDP 3");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tail_keeps_the_last_non_empty_lines() {
        assert_eq!(tail("a\n\nb\nc\n", 2), "b\nc");
        assert_eq!(tail("", 3), "");
    }
}
