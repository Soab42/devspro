//! The built-in way of running Mailpit, for machines without Docker: the same
//! single binary the Docker image wraps, downloaded once from Mailpit's GitHub
//! release and run as a plain process.
//!
//! Everything lives under `<app data>/mailcatcher/`:
//!
//! | | |
//! | --- | --- |
//! | `bin/mailpit(.exe)` | the downloaded binary — what Uninstall deletes |
//! | `mailpit.db` | caught mail, so it survives a restart like the container's did |
//! | `mailpit.pid` | which process is ours (see `PidFile`) |
//! | `mailpit.log` | its stdout/stderr, read back when it fails to start |
//!
//! **Owning the process is the point of the pid file.** DevsPro only stops what it
//! started, and a bare pid can't prove that: the OS hands pids out again, so after
//! a reboot the number in the file may belong to anything. A pid counts as ours only
//! while it is alive *and* runs the executable recorded next to it.
//!
//! The release is pinned, with each asset's sha256 taken from GitHub's release
//! metadata, so a download is checked against what was reviewed rather than
//! whatever `latest` points at today.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const VERSION: &str = "1.32.0";
pub const PROGRESS_EVENT: &str = "mailcatcher://install-progress";

/// (os, arch, asset, sha256) for every platform Mailpit publishes a build for.
const ASSETS: &[(&str, &str, &str, &str)] = &[
    ("windows", "x86_64", "mailpit-windows-amd64.zip", "16915767a6c95dca569336e8f561f91cf355e9a33c57b91deedcc10a4bba7fd4"),
    ("windows", "aarch64", "mailpit-windows-arm64.zip", "663a92d2741421ef33be6aff5eff1c1ad95579d66a41d6067d1209ff78951acb"),
    ("linux", "x86_64", "mailpit-linux-amd64.tar.gz", "4b5993c978e8604c70d43927e2ad103b311bba3b1822fa0cf1d4ee4ec890a93c"),
    ("linux", "aarch64", "mailpit-linux-arm64.tar.gz", "e1e45d1dc72bd497fb262a1e4100178eee057c42c5fc36d143eb8621769e1137"),
    ("linux", "x86", "mailpit-linux-386.tar.gz", "4be5cbb4b5342864a1995c431bdc8b3132fce0ddd0af63b227b658c668211082"),
    ("linux", "arm", "mailpit-linux-arm.tar.gz", "fa4cf9769eebe9718ac695c0eb428fdf6bc4c26c5b037081c69c4e94723e3ed5"),
    ("macos", "x86_64", "mailpit-darwin-amd64.tar.gz", "d7f3d0964cd2ea4c2a7d10660a12e02876718e67d88ced390d9cd90350a453da"),
    ("macos", "aarch64", "mailpit-darwin-arm64.tar.gz", "23709bd727f20da2290de6a2a40c4685c48a3127586d77370efd1a3a595f9ef6"),
];

const BINARY: &str = if cfg!(windows) { "mailpit.exe" } else { "mailpit" };

fn asset_for(os: &str, arch: &str) -> Option<(&'static str, &'static str)> {
    ASSETS.iter().find(|(o, a, _, _)| *o == os && *a == arch).map(|(_, _, name, sha)| (*name, *sha))
}

fn bin_path(dir: &Path) -> PathBuf {
    dir.join("bin").join(BINARY)
}

fn pid_path(dir: &Path) -> PathBuf {
    dir.join("mailpit.pid")
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("mailpit.log")
}

/// The binary to run: ours if it has been downloaded, otherwise one the user
/// installed themselves. Running theirs is still a process DevsPro started, so
/// it is still ours to stop — the pid file records whichever path it was.
pub fn binary(dir: &Path) -> Option<PathBuf> {
    let ours = bin_path(dir);
    if ours.is_file() {
        return Some(ours);
    }
    crate::modules::tools::find("mailpit").map(PathBuf::from)
}

pub fn downloaded(dir: &Path) -> bool {
    bin_path(dir).is_file()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PidFile {
    pub pid: u32,
    pub exe: String,
    pub smtp_port: u16,
    pub http_port: u16,
}

/// Same file, two spellings: Windows reports `C:\…` where we may hold `\\?\C:\…`,
/// and is case-insensitive about all of it.
fn same_file(a: &str, b: &str) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        }
    }
}

/// Our Mailpit, if one is running. A pid file that no longer describes a live
/// process of ours is stale — left by a crash or a reboot — and is deleted, so
/// the next look is a plain "not running".
pub fn running(dir: &Path) -> Option<PidFile> {
    let path = pid_path(dir);
    let text = fs::read_to_string(&path).ok()?;
    let ours = serde_json::from_str::<PidFile>(&text).ok().filter(|p| {
        crate::modules::processes::exe_path(p.pid).is_some_and(|exe| same_file(&exe, &p.exe))
    });
    if ours.is_none() {
        let _ = fs::remove_file(&path);
    }
    ours
}

/// Starts Mailpit bound to loopback only — the Docker version published on every
/// interface, which put a mail catcher on the LAN for no reason.
///
/// Started in its own process group (Unix) / console group (Windows) so a Ctrl+C
/// in the terminal DevsPro was launched from doesn't take it down. Whether it
/// outlives DevsPro is decided at exit by `keep_running`, not here, so flipping
/// that switch takes effect without a restart.
pub fn start(dir: &Path, smtp_port: u16, http_port: u16) -> Result<(Child, PidFile), String> {
    let exe = binary(dir).ok_or("Mailpit isn't installed yet.")?;
    let log = fs::File::create(log_path(dir)).map_err(|e| format!("could not create the Mailpit log: {e}"))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let mut command = crate::modules::tools::command(&exe);
    command
        .arg("--smtp")
        .arg(format!("127.0.0.1:{smtp_port}"))
        .arg("--listen")
        .arg(format!("127.0.0.1:{http_port}"))
        .arg("--database")
        .arg(dir.join("mailpit.db"))
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command.spawn().map_err(|e| format!("could not start Mailpit: {e}"))?;
    let pid_file = PidFile {
        pid: child.id(),
        exe: exe.to_string_lossy().to_string(),
        smtp_port,
        http_port,
    };
    // Written before waiting, so even a start that times out below leaves a
    // process DevsPro knows is its own and can stop.
    fs::write(pid_path(dir), serde_json::to_string(&pid_file).map_err(|e| e.to_string())?)
        .map_err(|e| format!("could not write the Mailpit pid file: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = fs::remove_file(pid_path(dir));
            return Err(format!("Mailpit exited ({status}) as it started: {}", log_tail(dir)));
        }
        if super::probe::listening(http_port) && super::probe::listening(smtp_port) {
            return Ok((child, pid_file));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(pid_path(dir));
            return Err(format!("Mailpit didn't start listening within 10 seconds: {}", log_tail(dir)));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn log_tail(dir: &Path) -> String {
    let text = fs::read_to_string(log_path(dir)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" / ");
    if tail.is_empty() {
        "it printed nothing".to_string()
    } else {
        tail
    }
}

/// Stops our Mailpit. `child` is the handle when this session started it — killing
/// through it also reaps the process; otherwise (started by an earlier session that
/// kept it running) it goes by pid, which `running` has just proved is ours.
pub fn stop(dir: &Path, pid_file: &PidFile, child: Option<Child>) -> Result<(), String> {
    match child {
        Some(mut child) if child.id() == pid_file.pid => {
            let _ = child.kill();
            let _ = child.wait();
        }
        _ => {
            crate::modules::processes::processes_kill(pid_file.pid, true)?;
        }
    }
    let _ = fs::remove_file(pid_path(dir));
    super::probe::wait_until_free(&[pid_file.smtp_port, pid_file.http_port], Duration::from_secs(5));
    Ok(())
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Downloads, verifies and unpacks the pinned release into `bin/`. Nothing is
/// started: installing and running are separate decisions the user makes.
pub async fn install(app: &tauri::AppHandle, dir: &Path) -> Result<(), String> {
    let (asset, sha256) = asset_for(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "Mailpit has no build for {} on {}.",
            std::env::consts::ARCH,
            std::env::consts::OS
        )
    })?;
    let url = format!("https://github.com/axllent/mailpit/releases/download/v{VERSION}/{asset}");

    let mut response = reqwest::get(&url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("could not download Mailpit: {e}"))?;
    let total = response.content_length();
    let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut last_emit = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("the Mailpit download broke off: {e}"))? {
        bytes.extend_from_slice(&chunk);
        let downloaded = bytes.len() as u64;
        if downloaded - last_emit >= 256 * 1024 || Some(downloaded) == total {
            last_emit = downloaded;
            let _ = app.emit(PROGRESS_EVENT, Progress { downloaded, total });
        }
    }

    let digest = Sha256::digest(&bytes);
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != sha256 {
        return Err(format!("the downloaded {asset} doesn't match its published checksum, so it was discarded"));
    }

    let dir = dir.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || unpack(&bytes, asset, &dir))
        .await
        .map_err(|e| e.to_string())?
}

fn unpack(archive: &[u8], asset: &str, dir: &Path) -> Result<(), String> {
    let binary = if asset.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| e.to_string())?;
        let mut entry = zip.by_name(BINARY).map_err(|_| format!("{asset} has no {BINARY} in it"))?;
        let mut out = Vec::new();
        entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
        out
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        let mut found = None;
        for entry in tar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let is_binary = entry.path().ok().and_then(|p| p.file_name().map(|n| n == BINARY)).unwrap_or(false);
            if is_binary {
                let mut out = Vec::new();
                entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
                found = Some(out);
                break;
            }
        }
        found.ok_or_else(|| format!("{asset} has no {BINARY} in it"))?
    };

    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    // Written aside and renamed into place, so an interrupted install never
    // leaves a half-written binary that `downloaded` would report as installed.
    let partial = bin_dir.join(format!("{BINARY}.partial"));
    {
        let mut file = fs::File::create(&partial).map_err(|e| e.to_string())?;
        file.write_all(&binary).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    fs::rename(&partial, bin_path(dir)).map_err(|e| format!("could not install Mailpit: {e}"))
}

/// Removes everything `install` and `start` left behind. Caught mail goes too
/// only when asked — it's the one thing here the user might want to keep.
pub fn uninstall(dir: &Path, delete_mail: bool) -> Result<(), String> {
    let bin_dir = dir.join("bin");
    if bin_dir.exists() {
        fs::remove_dir_all(&bin_dir).map_err(|e| format!("could not delete {}: {e}", bin_dir.display()))?;
    }
    let _ = fs::remove_file(pid_path(dir));
    let _ = fs::remove_file(log_path(dir));
    if delete_mail {
        for name in ["mailpit.db", "mailpit.db-wal", "mailpit.db-shm"] {
            let path = dir.join(name);
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("could not delete {}: {e}", path.display()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_desktop_platform_has_an_asset() {
        for (os, arch) in [("windows", "x86_64"), ("linux", "x86_64"), ("linux", "aarch64"), ("macos", "aarch64"), ("macos", "x86_64")] {
            assert!(asset_for(os, arch).is_some(), "{os}/{arch}");
        }
        assert!(asset_for("freebsd", "x86_64").is_none());
    }

    #[test]
    fn windows_ships_a_zip_and_the_rest_tarballs() {
        assert!(asset_for("windows", "x86_64").unwrap().0.ends_with(".zip"));
        assert!(asset_for("linux", "x86_64").unwrap().0.ends_with(".tar.gz"));
    }

    #[test]
    fn checksums_are_full_sha256_hex() {
        for (_, _, _, sha) in ASSETS {
            assert_eq!(sha.len(), 64);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn a_pid_file_for_a_process_that_isnt_ours_is_dropped() {
        let dir = std::env::temp_dir().join(format!("devspro-mailpit-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // Our own pid is alive, but its executable is the test binary, not this.
        let stale = PidFile { pid: std::process::id(), exe: "/nowhere/mailpit".into(), smtp_port: 1, http_port: 2 };
        fs::write(pid_path(&dir), serde_json::to_string(&stale).unwrap()).unwrap();
        assert_eq!(running(&dir), None);
        assert!(!pid_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pid_file_naming_the_live_executable_is_ours() {
        let dir = std::env::temp_dir().join(format!("devspro-mailpit-test-own-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let me = std::env::current_exe().unwrap().to_string_lossy().to_string();
        let live = PidFile { pid: std::process::id(), exe: me, smtp_port: 1, http_port: 2 };
        fs::write(pid_path(&dir), serde_json::to_string(&live).unwrap()).unwrap();
        assert_eq!(running(&dir), Some(live));
        let _ = fs::remove_dir_all(&dir);
    }
}
