//! In-app updates: check the website's feed, download the package for how this
//! copy was installed, and install it through the elevation helper.
//!
//! The feed is `latest.json` in the shape Tauri's own updater reads — version,
//! notes, pub_date, and per platform a url and a signature — so `tauri signer`
//! makes the signatures and `scripts/release.mjs` writes the file. Tauri's
//! updater plugin isn't used: it would run `pkexec dpkg -i` itself, around the
//! helper that every other root action goes through.
//!
//! The signature is checked twice. Here, as the download streams in, so a bad
//! file is rejected before anyone is asked for a password; and again by the
//! helper, as root, against a key compiled into it — that second check is the
//! one that matters, since it is what stops `InstallUpdate` from installing
//! anything else.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::elevate::{self, Operation, OperationManifest};

/// Published by the website build; `scripts/release.mjs` writes it.
pub const FEED_URL: &str = "https://soab42.github.io/devspro/latest.json";
pub const RELEASE_NOTES_URL: &str = "https://soab42.github.io/devspro/changelog.html";
/// The updater's public key as `tauri signer generate` writes it. The helper
/// compiles in the same file.
const PUBLIC_KEY: &str = include_str!("../../../updater.pub");

pub const AVAILABLE_EVENT: &str = "updater://available";
pub const PROGRESS_EVENT: &str = "updater://progress";
/// Settings key. Checking is on unless this is "false".
const AUTO_CHECK_KEY: &str = "updates_auto_check";
/// Long enough not to slow startup down, short enough to be seen.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
/// DevsPro often runs for days in the tray.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

#[derive(Debug, Deserialize)]
struct Feed {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    pub_date: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, Package>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Package {
    url: String,
    signature: String,
}

/// How this copy was installed, which decides which package an update needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    Deb,
    Rpm,
    Nsis,
}

impl InstallKind {
    /// The feed's platform key, named the way Tauri's updater names them.
    fn feed_key(self) -> String {
        let (os, installer) = match self {
            InstallKind::Deb => ("linux", "deb"),
            InstallKind::Rpm => ("linux", "rpm"),
            InstallKind::Nsis => ("windows", "nsis"),
        };
        format!("{os}-{}-{installer}", std::env::consts::ARCH)
    }

    fn extension(self) -> &'static str {
        match self {
            InstallKind::Deb => "deb",
            InstallKind::Rpm => "rpm",
            InstallKind::Nsis => "exe",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    /// The feed's version is newer than this one.
    pub available: bool,
    /// ...and the feed has a package for how this copy was installed.
    pub installable: bool,
    pub notes: String,
    pub pub_date: Option<String>,
    pub checked_at: String,
    #[serde(skip)]
    package: Option<Package>,
}

struct Downloaded {
    version: String,
    path: PathBuf,
    signature: String,
}

#[derive(Default)]
struct Inner {
    last_check: Option<UpdateCheck>,
    downloading: bool,
    downloaded: Option<Downloaded>,
}

#[derive(Default)]
pub struct UpdaterState {
    install: OnceLock<Result<InstallKind, String>>,
    inner: Mutex<Inner>,
}

impl UpdaterState {
    /// Worked out once: an update replaces the binary this asks about.
    fn install(&self, app: &AppHandle) -> Result<InstallKind, String> {
        self.install.get_or_init(|| detect_install(app)).clone()
    }
}

#[derive(Serialize)]
pub struct UpdaterStatus {
    current_version: String,
    install_kind: Option<InstallKind>,
    /// Why this copy can't install an update itself. `None` when it can.
    unsupported: Option<String>,
    auto_check: bool,
    last_check: Option<UpdateCheck>,
    downloading: bool,
    /// The version downloaded and verified, ready to install.
    ready: Option<String>,
    feed_url: &'static str,
    release_notes_url: &'static str,
}

#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn detect_install(app: &AppHandle) -> Result<InstallKind, String> {
    if cfg!(debug_assertions) {
        return Err("This is a development build. Updates install over a packaged copy of DevsPro.".to_string());
    }
    #[cfg(windows)]
    {
        let _ = app;
        Ok(InstallKind::Nsis)
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        let exe = tauri::process::current_binary(&app.env()).map_err(|e| e.to_string())?;
        let owned_by = |program: &str, flag: &str| {
            Command::new(program)
                .arg(flag)
                .arg(&exe)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        if owned_by("dpkg-query", "-S") {
            Ok(InstallKind::Deb)
        } else if owned_by("rpm", "-qf") {
            Ok(InstallKind::Rpm)
        } else {
            Err("DevsPro wasn't installed from its .deb or .rpm package, so it can't update itself. Download the new version from the website instead.".to_string())
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = app;
        Err("Updates aren't available on this platform yet.".to_string())
    }
}

fn evaluate(
    feed: Feed,
    current: &semver::Version,
    install: Option<InstallKind>,
    checked_at: String,
) -> Result<UpdateCheck, String> {
    let latest = semver::Version::parse(feed.version.trim().trim_start_matches('v'))
        .map_err(|e| format!("the update feed has an unreadable version {:?}: {e}", feed.version))?;
    let available = latest > *current;
    let package = install.and_then(|kind| feed.platforms.get(&kind.feed_key()).cloned());
    Ok(UpdateCheck {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        available,
        installable: available && package.is_some(),
        notes: feed.notes,
        pub_date: feed.pub_date,
        checked_at,
        package,
    })
}

/// The package's file name from its URL, if it is a plain one with the right
/// extension; a fixed name otherwise. It names a file in our own cache folder.
fn package_file_name(url: &str, kind: InstallKind) -> String {
    let last = url.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
    let plain = !last.is_empty()
        && !last.starts_with('.')
        && last.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if plain && last.ends_with(&format!(".{}", kind.extension())) {
        last.to_string()
    } else {
        format!("DevsPro-update.{}", kind.extension())
    }
}

/// Both in the base64 form `tauri signer` writes. The helper decodes them the
/// same way (`helper/src/main.rs`); the two crates share no code by design.
fn decode_key_and_signature(
    public_key: &str,
    signature: &str,
) -> Result<(minisign_verify::PublicKey, minisign_verify::Signature), String> {
    use base64::Engine;
    let decode = |b64: &str, what: &str| -> Result<String, String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| format!("{what} is not valid base64: {e}"))?;
        String::from_utf8(raw).map_err(|_| format!("{what} is not text"))
    };
    let key = minisign_verify::PublicKey::decode(&decode(public_key, "the update key")?)
        .map_err(|e| format!("the update key is malformed: {e}"))?;
    let sig = minisign_verify::Signature::decode(&decode(signature, "the signature")?)
        .map_err(|e| format!("the update's signature is malformed: {e}"))?;
    Ok((key, sig))
}

/// The `x.y.z` a release file's name carries, read from the `file:` that
/// `tauri signer sign` writes into the signed trusted comment
/// (`DevsPro_0.2.0_amd64.deb`, `DevsPro-0.2.0-1.x86_64.rpm`,
/// `DevsPro_0.2.0_x64-setup.exe`). The helper reads it the same way.
fn signed_version(trusted_comment: &str) -> Option<(u64, u64, u64)> {
    let file = trusted_comment.split('\t').find_map(|field| field.strip_prefix("file:"))?;
    let rest = file.strip_prefix("DevsPro")?;
    let rest = rest.strip_prefix('_').or_else(|| rest.strip_prefix('-'))?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = digits.split('.').filter(|p| !p.is_empty()).map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// The feed's version is just text on a web page; the signature is what can be
/// trusted. So the package must be signed under the version the feed promised —
/// otherwise a tampered feed could offer an old release as a new one. The
/// comment is only proven genuine once the download verifies, which `finalize`
/// checks; this rejects a mismatch before downloading anything.
fn check_signed_as(signature: &minisign_verify::Signature, version: &str) -> Result<(), String> {
    let expected = semver::Version::parse(version).map_err(|e| e.to_string())?;
    match signed_version(signature.trusted_comment()) {
        Some(signed) if signed == (expected.major, expected.minor, expected.patch) => Ok(()),
        _ => Err(format!(
            "the update feed offers {version}, but its package isn't signed as {version} — not downloading it"
        )),
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("DevsPro/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

async fn check(app: &AppHandle) -> Result<UpdateCheck, String> {
    let state = app.state::<UpdaterState>();
    let response = http_client()?
        .get(FEED_URL)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("couldn't reach the update feed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("the update feed answered {}", response.status()));
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| format!("couldn't read the update feed: {e}"))?;
    let feed: Feed = serde_json::from_slice(&body).map_err(|e| format!("the update feed isn't valid: {e}"))?;
    let current = app.package_info().version.clone();
    let result = evaluate(feed, &current, state.install(app).ok(), chrono::Utc::now().to_rfc3339())?;
    state.inner.lock().map_err(|e| e.to_string())?.last_check = Some(result.clone());
    if result.available {
        let _ = app.emit(AVAILABLE_EVENT, &result);
    }
    Ok(result)
}

fn auto_check_enabled(app: &AppHandle) -> bool {
    let db = app.state::<crate::db::Db>();
    let value = db.0.lock().ok().and_then(|conn| super::settings::get(&conn, AUTO_CHECK_KEY));
    value.as_deref() != Some("false")
}

/// Checks a little after startup and then every few hours, while the app runs.
/// A development build never checks on its own.
pub fn start_background_checks(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            if auto_check_enabled(&app) {
                let _ = check(&app).await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[tauri::command(async)]
pub fn updater_status(app: AppHandle, state: State<UpdaterState>) -> Result<UpdaterStatus, String> {
    let install = state.install(&app);
    let inner = state.inner.lock().map_err(|e| e.to_string())?;
    Ok(UpdaterStatus {
        current_version: app.package_info().version.to_string(),
        install_kind: install.as_ref().ok().copied(),
        unsupported: install.err(),
        auto_check: auto_check_enabled(&app),
        last_check: inner.last_check.clone(),
        downloading: inner.downloading,
        ready: inner.downloaded.as_ref().filter(|d| d.path.is_file()).map(|d| d.version.clone()),
        feed_url: FEED_URL,
        release_notes_url: RELEASE_NOTES_URL,
    })
}

#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<UpdateCheck, String> {
    check(&app).await
}

/// Clears `downloading` however the download ends.
struct DownloadingFlag<'a>(&'a UpdaterState);

impl Drop for DownloadingFlag<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.0.inner.lock() {
            inner.downloading = false;
        }
    }
}

/// Downloads the package the last check found, checking its signature as it
/// streams in. Emits `PROGRESS_EVENT` along the way. Returns the version that
/// is now ready to install.
#[tauri::command]
pub async fn updater_download(app: AppHandle) -> Result<String, String> {
    let state = app.state::<UpdaterState>();
    let kind = state.install(&app)?;
    let (version, package) = {
        let mut inner = state.inner.lock().map_err(|e| e.to_string())?;
        let check = inner.last_check.as_ref().ok_or("check for updates first")?;
        let package = check.package.clone().filter(|_| check.installable).ok_or("there is no update to download")?;
        let version = check.latest_version.clone();
        if inner.downloaded.as_ref().is_some_and(|d| d.version == version && d.path.is_file()) {
            return Ok(version);
        }
        if inner.downloading {
            return Err("the update is already downloading".to_string());
        }
        inner.downloading = true;
        (version, package)
    };
    let _flag = DownloadingFlag(&state);

    let (key, signature) = decode_key_and_signature(PUBLIC_KEY, &package.signature)?;
    check_signed_as(&signature, &version)?;
    let mut verifier = key
        .verify_stream(&signature)
        .map_err(|e| format!("the update's signature can't be checked: {e}"))?;

    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    // Only the newest download is ever wanted.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let path = dir.join(package_file_name(&package.url, kind));
    let partial = path.with_extension("part");

    let result = async {
        let mut response = http_client()?
            .get(&package.url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("couldn't download the update: {e}"))?;
        let total = response.content_length();
        let mut file = std::fs::File::create(&partial).map_err(|e| format!("couldn't write the update: {e}"))?;
        let mut downloaded = 0u64;
        let mut last_emit = Instant::now();
        let _ = app.emit(PROGRESS_EVENT, Progress { downloaded, total });
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("the download was interrupted: {e}"))?
        {
            verifier.update(&chunk);
            file.write_all(&chunk).map_err(|e| format!("couldn't write the update: {e}"))?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed() >= PROGRESS_EVERY {
                last_emit = Instant::now();
                let _ = app.emit(PROGRESS_EVENT, Progress { downloaded, total });
            }
        }
        file.sync_all().map_err(|e| format!("couldn't write the update: {e}"))?;
        let _ = app.emit(PROGRESS_EVENT, Progress { downloaded, total });
        verifier
            .finalize()
            .map_err(|e| format!("the download doesn't match its signature, so it was discarded: {e}"))?;
        std::fs::rename(&partial, &path).map_err(|e| format!("couldn't keep the update: {e}"))
    }
    .await;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }

    state.inner.lock().map_err(|e| e.to_string())?.downloaded = Some(Downloaded {
        version: version.clone(),
        path,
        signature: package.signature,
    });
    Ok(version)
}

/// Installs the downloaded update through the helper — one password or UAC
/// prompt — and then restarts DevsPro. On Linux the package manager has
/// replaced the binary by the time the helper returns, so the app restarts
/// itself. On Windows the helper has only started the installer, which needs
/// DevsPro gone before it can replace it and starts it again when done.
#[tauri::command(async)]
pub fn updater_install(app: AppHandle, state: State<UpdaterState>) -> Result<(), String> {
    let kind = state.install(&app)?;
    let (path, signature) = {
        let inner = state.inner.lock().map_err(|e| e.to_string())?;
        let d = inner
            .downloaded
            .as_ref()
            .filter(|d| d.path.is_file())
            .ok_or("download the update first")?;
        (d.path.clone(), d.signature.clone())
    };
    let results = elevate::run(
        &app,
        OperationManifest {
            allowed_write_prefix: String::new(),
            operations: vec![Operation::InstallUpdate {
                package_path: path.to_string_lossy().into_owned(),
                signature,
            }],
        },
    )?;
    match results.first() {
        Some(r) if r.success => {}
        Some(r) => return Err(r.message.clone()),
        None => return Err("the helper returned no result".to_string()),
    }
    // The helper worked from its own copy.
    let _ = std::fs::remove_file(&path);
    if kind == InstallKind::Nsis {
        app.exit(0);
    } else {
        app.request_restart();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(version: &str, keys: &[&str]) -> Feed {
        Feed {
            version: version.to_string(),
            notes: "### New\n- A thing".to_string(),
            pub_date: Some("2026-11-01T00:00:00Z".to_string()),
            platforms: keys
                .iter()
                .map(|k| (k.to_string(), Package { url: format!("https://example.com/{k}"), signature: "sig".to_string() }))
                .collect(),
        }
    }

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn only_a_newer_version_with_a_package_for_this_install_is_installable() {
        let deb = InstallKind::Deb.feed_key();
        let rpm = InstallKind::Rpm.feed_key();
        let newer = evaluate(feed("0.2.0", &[&deb, &rpm]), &v("0.1.1"), Some(InstallKind::Deb), String::new()).unwrap();
        assert!(newer.available && newer.installable);
        assert_eq!(newer.package.unwrap().url, format!("https://example.com/{deb}"));

        let same = evaluate(feed("0.1.1", &[&deb]), &v("0.1.1"), Some(InstallKind::Deb), String::new()).unwrap();
        assert!(!same.available && !same.installable);
        let older = evaluate(feed("0.1.0", &[&deb]), &v("0.1.1"), Some(InstallKind::Deb), String::new()).unwrap();
        assert!(!older.available);

        // Newer, but only built for the other package format, or no install kind known.
        let rpm_only = evaluate(feed("0.2.0", &[&rpm]), &v("0.1.1"), Some(InstallKind::Deb), String::new()).unwrap();
        assert!(rpm_only.available && !rpm_only.installable);
        let dev = evaluate(feed("0.2.0", &[&deb]), &v("0.1.1"), None, String::new()).unwrap();
        assert!(dev.available && !dev.installable);
    }

    #[test]
    fn versions_compare_as_semver_and_may_carry_a_v() {
        let check = evaluate(feed("v0.10.0", &[]), &v("0.9.3"), None, String::new()).unwrap();
        assert_eq!(check.latest_version, "0.10.0");
        assert!(check.available);
        // A pre-release is older than the release it leads up to.
        assert!(!evaluate(feed("0.2.0-beta.1", &[]), &v("0.2.0"), None, String::new()).unwrap().available);
        assert!(evaluate(feed("latest", &[]), &v("0.1.0"), None, String::new()).is_err());
    }

    #[test]
    fn feed_keys_follow_tauris_naming() {
        let arch = std::env::consts::ARCH;
        assert_eq!(InstallKind::Deb.feed_key(), format!("linux-{arch}-deb"));
        assert_eq!(InstallKind::Rpm.feed_key(), format!("linux-{arch}-rpm"));
        assert_eq!(InstallKind::Nsis.feed_key(), format!("windows-{arch}-nsis"));
    }

    #[test]
    fn the_feed_parses_in_the_shape_release_mjs_writes() {
        let raw = r####"{"version":"0.2.0","notes":"### New\n- Updates","pub_date":"2026-11-01T00:00:00Z",
            "platforms":{"linux-x86_64-deb":{"url":"https://github.com/Soab42/devspro/releases/download/v0.2.0/DevsPro_0.2.0_amd64.deb","signature":"c2ln"}}}"####;
        let feed: Feed = serde_json::from_str(raw).unwrap();
        assert_eq!(feed.platforms["linux-x86_64-deb"].signature, "c2ln");
        // The seed the site starts with has no platforms at all.
        let seed: Feed = serde_json::from_str(r#"{"version":"0.1.1","notes":"","pub_date":"2026-10-05T00:00:00Z","platforms":{}}"#).unwrap();
        assert!(seed.platforms.is_empty());
    }

    #[test]
    fn package_file_names_come_from_the_url_only_when_plain() {
        let gh = "https://github.com/Soab42/devspro/releases/download/v0.2.0/DevsPro_0.2.0_amd64.deb";
        assert_eq!(package_file_name(gh, InstallKind::Deb), "DevsPro_0.2.0_amd64.deb");
        assert_eq!(package_file_name("https://x.test/DevsPro_0.2.0_x64-setup.exe?sig=1", InstallKind::Nsis), "DevsPro_0.2.0_x64-setup.exe");
        for odd in ["https://x.test/a%20b.deb", "https://x.test/", "https://x.test/pkg.rpm", "https://x.test/.deb", "https://x.test/..%2f.deb"] {
            assert_eq!(package_file_name(odd, InstallKind::Deb), "DevsPro-update.deb", "{odd}");
        }
    }

    // A throwaway key and its signature of TEST_PACKAGE, as in the helper's tests.
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDY3QjIwQjg0QTA1NjBCQzEKUldUQkMxYWdoQXV5WjRlWGpUelg3aTlyYllRRTRTb21SNkx2QldvSC91dkM1WUhtUlVVS3FQNXoK";
    const TEST_PACKAGE: &[u8] = b"devspro update fixture";
    const TEST_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUQkMxYWdoQXV5Wis4YVlPWEk1MXQyZ3RacFZtQXh0M2RoeEVzL1QzZjZBUDFjU01JQkJPbnBYVm9EYXlSakNvMXVFdmcxK0c0V3kzdWFvWDlQNjlRTkd3V0dHY1NDcXdrPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzMjM4ODQzCWZpbGU6Zml4dHVyZS5kZWIKSmhyaW9mODd6WXVqaFZIeURxVmd6YnkzWHFqZkduWFlGSWxIV2g5WW85ZTBZeWdvZ1FFeGUxbEkzQVhrM1hqVnJZUFAvN2s5T2FQQzNhOWw5WFNBQ3c9PQo=";

    fn verify_in_chunks(key: &str, data: &[u8], sig: &str) -> Result<(), String> {
        let (key, sig) = decode_key_and_signature(key, sig)?;
        let mut verifier = key.verify_stream(&sig).map_err(|e| e.to_string())?;
        for chunk in data.chunks(5) {
            verifier.update(chunk);
        }
        verifier.finalize().map_err(|e| e.to_string())
    }

    #[test]
    fn a_download_verifies_chunk_by_chunk_only_against_its_own_signature() {
        assert_eq!(verify_in_chunks(TEST_KEY, TEST_PACKAGE, TEST_SIGNATURE), Ok(()));
        assert!(verify_in_chunks(TEST_KEY, b"devspro update fixturE", TEST_SIGNATURE).is_err());
        // The shipped key didn't sign the fixture.
        assert!(verify_in_chunks(PUBLIC_KEY, TEST_PACKAGE, TEST_SIGNATURE).is_err());
    }

    // TEST_PACKAGE signed as DevsPro_9.9.9_amd64.deb (the helper's tests use it too).
    const SIGNED_AS_9_9_9: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUQkMxYWdoQXV5WjJZYlRUeFQzWHNYOS8rOGRPcW91eklhMkg5WXNPM1pCc2NXQi96VUNwaEZPMmZibnlzTnU4aVlSV1NVdUtJaW1xek9hR2c2d0RVQ2hMRGh3V1JZemcwPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzMjQxNDEwCWZpbGU6RGV2c1Byb185LjkuOV9hbWQ2NC5kZWIKUnkrMXJEVUNTc0hYamZ1OEdXMG9ISEdKYk5jdGpQWk9tdEpCNzhlcGdPUmRidmpkUmQramJodGtTR1AxekVxaEZ0VlVCL0lFdi8rTWZwZGY2clMvQnc9PQo=";

    #[test]
    fn a_package_must_be_signed_as_the_version_the_feed_offers() {
        let (_, sig) = decode_key_and_signature(TEST_KEY, SIGNED_AS_9_9_9).unwrap();
        assert_eq!(check_signed_as(&sig, "9.9.9"), Ok(()));
        assert!(check_signed_as(&sig, "10.0.0").unwrap_err().contains("isn't signed as 10.0.0"));
        let (_, unnamed) = decode_key_and_signature(TEST_KEY, TEST_SIGNATURE).unwrap();
        assert!(check_signed_as(&unnamed, "9.9.9").is_err());
        assert_eq!(signed_version("timestamp:1\tfile:DevsPro-0.10.3-1.x86_64.rpm"), Some((0, 10, 3)));
        assert_eq!(signed_version("timestamp:1\tfile:DevsPro_1.0.0_x64-setup.exe"), Some((1, 0, 0)));
        assert_eq!(signed_version("timestamp:1\tfile:Other_1.0.0_x64-setup.exe"), None);
    }

    #[test]
    fn the_shipped_key_is_a_well_formed_minisign_key() {
        use base64::Engine;
        let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY.trim()).unwrap()).unwrap();
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
    }
}
