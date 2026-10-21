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
//!
//! On Windows the client is usually the built-in Remote Desktop Connection,
//! which takes a `.rdp` file and finds the password in Credential Manager
//! rather than on its command line (see `mstsc.rs`); a Windows build of
//! FreeRDP is offered too when one is installed.

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

use super::connections::RemoteDesktop;

pub const EXIT_EVENT: &str = "remote-desktop:rdp-exit";

/// Known clients in order of preference. FreeRDP 3 first: it is the current
/// series and the one modern distributions package.
#[cfg(unix)]
const CANDIDATES: [(&str, &str); 5] = [
    ("xfreerdp3", "FreeRDP 3"),
    ("xfreerdp", "FreeRDP 2"),
    ("wlfreerdp", "FreeRDP (Wayland)"),
    ("remmina", "Remmina"),
    ("rdesktop", "rdesktop"),
];

/// On Windows, the client every edition ships first, then FreeRDP's Windows
/// builds (3.x, so they take the same flags as `xfreerdp3`).
#[cfg(windows)]
const CANDIDATES: [(&str, &str); 4] = [
    ("mstsc", "Remote Desktop Connection"),
    ("wfreerdp", "FreeRDP"),
    ("sdl-freerdp", "FreeRDP (SDL)"),
    ("sdl3-freerdp", "FreeRDP (SDL3)"),
];

/// What the launch dialog says about the password field for a client.
const PASSWORD_ON_COMMAND_LINE: &str = "Passed to the client for this launch only. It is visible to other users of this machine in the process list while the client runs; leave it empty to let the client ask.";
#[cfg(windows)]
const PASSWORD_IN_CREDENTIAL_MANAGER: &str = "Saved in Windows Credential Manager for this host while the session runs, then removed (a password you had already saved for it is put back). Leave it empty to let Remote Desktop Connection ask.";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RdpClient {
    /// The executable name; what the launch command is given back.
    pub id: String,
    pub label: String,
    pub path: String,
    /// How the password is handed over, for the field's description.
    pub password_note: String,
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

/// The file names `name` can have on disk: itself on Unix, with an
/// executable extension on Windows.
#[cfg(unix)]
fn file_names(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

#[cfg(windows)]
fn file_names(name: &str) -> Vec<String> {
    vec![format!("{name}.exe"), format!("{name}.com")]
}

fn find_on_path(name: &str, path_var: &str) -> Option<PathBuf> {
    let names = file_names(name);
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| names.iter().map(|n| dir.join(n)).collect::<Vec<_>>())
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn password_note(_client: &str) -> &'static str {
    PASSWORD_ON_COMMAND_LINE
}

#[cfg(windows)]
fn password_note(client: &str) -> &'static str {
    if client == "mstsc" {
        PASSWORD_IN_CREDENTIAL_MANAGER
    } else {
        PASSWORD_ON_COMMAND_LINE
    }
}

/// Where to look for a client: `PATH`, plus on Windows the folders FreeRDP's
/// installers use, which they don't add to `PATH`.
fn search_path() -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    #[cfg(windows)]
    {
        let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(root) = std::env::var(var) {
                dirs.push(Path::new(&root).join("FreeRDP"));
                dirs.push(Path::new(&root).join("FreeRDP").join("bin"));
            }
        }
        if let Ok(joined) = std::env::join_paths(dirs) {
            return joined.to_string_lossy().into_owned();
        }
    }
    path
}

pub fn detect(path_var: &str) -> Vec<RdpClient> {
    CANDIDATES
        .iter()
        .filter_map(|(id, label)| {
            find_on_path(id, path_var).map(|path| RdpClient {
                id: id.to_string(),
                label: label.to_string(),
                path: path.to_string_lossy().into_owned(),
                password_note: password_note(id).to_string(),
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
        "xfreerdp3" | "xfreerdp" | "wlfreerdp" | "wfreerdp" | "sdl-freerdp" | "sdl3-freerdp" => {
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
    detect(&search_path())
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
    let installed = detect(&search_path());
    let Some(found) = installed.into_iter().find(|c| c.id == client) else {
        return Err(format!("{client} is not installed (or not on PATH)"));
    };

    let (args, cleanup) = prepare(&found.id, &desktop, password.as_deref())?;
    let mut command = Command::new(&found.path);
    command.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // The SDL builds are console programs; without this each launch
        // brings up an empty console next to the session window.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            cleanup();
            return Err(format!("failed to start {}: {e}", found.label));
        }
    };
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
        cleanup();
        let _ = app.emit(EXIT_EVENT, ExitEvent { pid, client: label, code, output: tail(&output, 12) });
    });

    Ok(RdpLaunch { pid, client: found.label })
}

/// Undoes whatever a launch set up, once the client has exited.
type Cleanup = Box<dyn FnOnce() + Send>;

/// The client's arguments, plus whatever must be undone when it exits.
#[cfg(unix)]
fn prepare(client: &str, desktop: &RemoteDesktop, password: Option<&str>) -> Result<(Vec<String>, Cleanup), String> {
    Ok((arguments(client, desktop, password), Box::new(|| {})))
}

/// mstsc gets a `.rdp` file and Credential Manager instead of arguments; the
/// FreeRDP builds are launched exactly as on Linux.
#[cfg(windows)]
fn prepare(client: &str, desktop: &RemoteDesktop, password: Option<&str>) -> Result<(Vec<String>, Cleanup), String> {
    if client != "mstsc" {
        return Ok((arguments(client, desktop, password), Box::new(|| {})));
    }
    let prepared = super::mstsc::prepare(desktop, password, &std::env::temp_dir().join("devspro-rdp"))?;
    let args = prepared.args.clone();
    Ok((args, Box::new(move || prepared.cleanup())))
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

    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn detect_finds_exe_files_on_a_semicolon_path_in_preference_order() {
        let dir = std::env::temp_dir().join(format!("devspro-rdp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wfreerdp.exe"), "").unwrap();
        std::fs::write(dir.join("mstsc.exe"), "").unwrap();
        // Without an executable extension it is not a program on Windows.
        std::fs::write(dir.join("sdl-freerdp"), "").unwrap();

        let found = detect(&format!("C:\\nonexistent;;{}", dir.display()));
        assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["mstsc", "wfreerdp"]);
        assert_eq!(found[0].path, dir.join("mstsc.exe").to_string_lossy());
        assert_eq!(found[0].password_note, PASSWORD_IN_CREDENTIAL_MANAGER);
        assert_eq!(found[1].password_note, PASSWORD_ON_COMMAND_LINE);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn mstsc_is_found_on_a_stock_windows_path() {
        let found = detect(&search_path());
        assert!(found.iter().any(|c| c.id == "mstsc"), "{found:?}");
    }

    #[test]
    fn windows_freerdp_builds_take_freerdp3_flags() {
        for client in ["wfreerdp", "sdl-freerdp", "sdl3-freerdp"] {
            assert_eq!(arguments(client, &desktop(false, ""), None), arguments("xfreerdp3", &desktop(false, ""), None));
        }
    }

    #[test]
    fn tail_keeps_the_last_non_empty_lines() {
        assert_eq!(tail("a\n\nb\nc\n", 2), "b\nc");
        assert_eq!(tail("", 3), "");
    }
}
