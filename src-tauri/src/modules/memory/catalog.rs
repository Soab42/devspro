//! The app catalog: the apps the user told DevsPro how to find, stop and start.
//! Stored as one JSON array under the `memory.apps` setting. Until the user
//! saves, it is killer's own `APPS` list, translated — returned on every read
//! but never written, so a later change to the seed still reaches anyone who
//! hasn't customised theirs.
//!
//! Also home to the auto-categorisation of detected apps (killer's `AUTO_CAT`).
//!
//! Windows gets a seed of its own — killer's paths, Flatpaks and systemd units
//! mean nothing there — and refuses Flatpak entries, which it can't run.

use super::protect::valid_unit_name;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const APPS_KEY: &str = "memory.apps";

/// What a service's `service` field holds, as validation messages call it.
#[cfg(not(windows))]
const UNIT_NOUN: &str = "systemd unit name";
#[cfg(windows)]
const UNIT_NOUN: &str = "service name";

pub const CATEGORIES: [&str; 7] = ["Browsers", "Communication", "Dev", "Creative", "Servers", "Background", "Other"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    /// Found and stopped by its command line (SIGTERM, then SIGKILL).
    Process,
    /// Stopped with `flatpak kill`, started with `flatpak run`.
    Flatpak,
    /// A systemd unit, started and stopped through the elevated helper.
    Service,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppDef {
    /// Stable slug, unique, `[a-z0-9-]+`.
    pub id: String,
    pub name: String,
    /// One of `CATEGORIES`.
    pub category: String,
    pub kind: AppKind,
    /// Substring of a process's full command line — killer's `match`. For every
    /// kind this is how RAM and CPU are measured; for `Process` it is also what
    /// gets stopped.
    pub matcher: String,
    /// `Process`: a shell command, run detached. Optional — without one the app
    /// can only be stopped. Ignored for other kinds.
    pub start: Option<String>,
    /// `Flatpak`: the application id.
    pub flatpak_id: Option<String>,
    /// `Service`: the systemd unit (on Windows, the service name).
    pub service: Option<String>,
}

/// A `Process` entry with nothing to start it by: it can still be stopped.
#[cfg(windows)]
fn stop_only(id: &str, name: &str, category: &str, matcher: &str) -> AppDef {
    AppDef {
        start: None,
        ..process(id, name, category, matcher, "")
    }
}

fn process(id: &str, name: &str, category: &str, matcher: &str, start: &str) -> AppDef {
    AppDef {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        kind: AppKind::Process,
        matcher: matcher.to_string(),
        start: Some(start.to_string()),
        flatpak_id: None,
        service: None,
    }
}

/// killer's flatpak entries all match on the app id, except Teams, whose
/// processes are named after the upstream project instead.
#[cfg(not(windows))]
fn flatpak(id: &str, name: &str, category: &str, matcher: &str, flatpak_id: &str) -> AppDef {
    AppDef {
        id: id.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        kind: AppKind::Flatpak,
        matcher: matcher.to_string(),
        start: None,
        flatpak_id: Some(flatpak_id.to_string()),
        service: None,
    }
}

fn service(id: &str, name: &str, matcher: &str, unit: &str) -> AppDef {
    AppDef {
        id: id.to_string(),
        name: name.to_string(),
        category: "Servers".to_string(),
        kind: AppKind::Service,
        matcher: matcher.to_string(),
        start: None,
        flatpak_id: None,
        service: Some(unit.to_string()),
    }
}

/// The Windows counterpart of killer's list: the same kinds of app, matched on
/// their install folders (case doesn't matter on Windows). Only apps a bare name
/// reliably starts — on PATH, or registered under App Paths, which `start` reads
/// — get a start command; the rest can be stopped, and given one in the editor.
/// The services are the names the MySQL and Apache Lounge installers register.
#[cfg(windows)]
pub fn seed() -> Vec<AppDef> {
    vec![
        process("chrome", "Chrome", "Browsers", r"\Google\Chrome\Application\chrome.exe", "chrome"),
        process("edge", "Edge", "Browsers", r"\Microsoft\Edge\Application\msedge.exe", "msedge"),
        process("firefox", "Firefox", "Browsers", r"\Mozilla Firefox\firefox.exe", "firefox"),
        stop_only("teams", "Teams", "Communication", "ms-teams.exe"),
        stop_only("discord", "Discord", "Communication", r"\Discord\app-"),
        stop_only("slack", "Slack", "Communication", "slack.exe"),
        stop_only("zoom", "Zoom", "Communication", r"\Zoom\bin\Zoom.exe"),
        stop_only("telegram", "Telegram", "Communication", "Telegram.exe"),
        process("thunderbird", "Thunderbird", "Communication", "thunderbird.exe", "thunderbird"),
        process("vscode", "VS Code", "Dev", r"\Microsoft VS Code\Code.exe", "code"),
        stop_only("postman", "Postman", "Dev", "Postman.exe"),
        stop_only("obs-studio", "OBS Studio", "Creative", "obs64.exe"),
        stop_only("gimp", "GIMP", "Creative", r"\bin\gimp-"),
        process("vlc", "VLC", "Creative", "vlc.exe", "vlc"),
        process("libreoffice", "LibreOffice", "Creative", "soffice", "soffice"),
        service("apache", "Apache", "httpd.exe", "Apache2.4"),
        stop_only("php-dev-server", "PHP Dev Server", "Servers", "php -S"),
        service("mysql", "MySQL", "mysqld", "MySQL80"),
        stop_only("dropbox", "Dropbox", "Background", "Dropbox.exe"),
        stop_only("onedrive", "OneDrive", "Background", "OneDrive.exe"),
        stop_only("claude-desktop", "Claude Desktop", "Background", r"\AnthropicClaude\"),
    ]
}

/// killer's `APPS`, in its order. `start_glob` (Dropbox) needs no special case:
/// the start command runs under `sh -c`, which expands the glob itself.
#[cfg(not(windows))]
pub fn seed() -> Vec<AppDef> {
    vec![
        process("chrome", "Chrome", "Browsers", "/opt/google/chrome/chrome", "/opt/google/chrome/chrome"),
        process("firefox", "Firefox", "Browsers", "firefox", "firefox"),
        flatpak("teams", "Teams", "Communication", "teams-for-linux", "com.github.IsmaelMartinez.teams_for_linux"),
        flatpak("discord", "Discord", "Communication", "com.discordapp.Discord", "com.discordapp.Discord"),
        process("zoom", "Zoom", "Communication", "/usr/bin/zoom", "/usr/bin/zoom"),
        flatpak("telegram", "Telegram", "Communication", "org.telegram.desktop", "org.telegram.desktop"),
        process("thunderbird", "Thunderbird", "Communication", "thunderbird", "thunderbird"),
        flatpak("bluemail", "BlueMail", "Communication", "net.blix.BlueMail", "net.blix.BlueMail"),
        flatpak("postman", "Postman", "Dev", "com.getpostman.Postman", "com.getpostman.Postman"),
        flatpak("zed", "Zed", "Dev", "dev.zed.Zed", "dev.zed.Zed"),
        flatpak("obs-studio", "OBS Studio", "Creative", "com.obsproject.Studio", "com.obsproject.Studio"),
        flatpak("kdenlive", "Kdenlive", "Creative", "org.kde.kdenlive", "org.kde.kdenlive"),
        flatpak("gimp", "GIMP", "Creative", "org.gimp.GIMP", "org.gimp.GIMP"),
        process("libreoffice", "LibreOffice", "Creative", "soffice", "libreoffice"),
        flatpak("vlc", "VLC", "Creative", "org.videolan.VLC", "org.videolan.VLC"),
        service("nginx", "Nginx", "nginx", "nginx"),
        service("apache2", "Apache2", "apache2", "apache2"),
        service("php-fpm-8-3", "PHP-FPM 8.3", "php-fpm", "php8.3-fpm"),
        process(
            "php-dev-server",
            "PHP Dev Server",
            "Servers",
            "php -S",
            // killer ran this through `sudo -u dhcpcd`, which has no terminal to
            // ask for a password on when DevsPro starts it in the background.
            "php -S [::]:8080 -t /var/www/html",
        ),
        service("mysql", "MySQL", "mysqld", "mysql"),
        process("dropbox", "Dropbox", "Background", "dropbox", "~/.dropbox-dist/dropbox-lnx.*/dropbox start"),
        process("claude-desktop", "Claude Desktop", "Background", "claude-desktop", "claude-desktop"),
        flatpak("alpaca", "Alpaca", "Background", "com.jeffser.Alpaca", "com.jeffser.Alpaca"),
        flatpak(
            "standard-notes",
            "Standard Notes",
            "Background",
            "org.standardnotes.standardnotes",
            "org.standardnotes.standardnotes",
        ),
    ]
}

/// The saved catalog, or the seed when nothing is saved. An unreadable value
/// falls back to the seed too, rather than leaving the watchdog with nothing.
pub fn load(conn: &Connection) -> Vec<AppDef> {
    match crate::modules::settings::get(conn, APPS_KEY) {
        None => seed(),
        Some(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            eprintln!("memory: {APPS_KEY} is not a valid app list ({e}); using the built-in one");
            seed()
        }),
    }
}

pub fn save(conn: &Connection, apps: &[AppDef]) -> Result<(), String> {
    let json = serde_json::to_string(apps).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (APPS_KEY, &json),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn trimmed(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn valid_flatpak_id(id: &str) -> bool {
    id.contains('.')
        && !id.starts_with('-')
        && !id.starts_with('.')
        && id.len() <= 255
        && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Checks a catalog before it is saved, and returns it tidied: strings trimmed,
/// empty optional fields turned into `None`. Fields for other kinds are kept,
/// so switching an entry's kind back and forth in the editor loses nothing.
///
/// The matcher must be at least three characters: for a `Process` app it
/// decides what gets signalled, and a one-letter matcher would hit nearly every
/// process on the machine.
pub fn validate(apps: Vec<AppDef>) -> Result<Vec<AppDef>, String> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(apps.len());
    for app in apps {
        let id = app.id.trim().to_string();
        let name = app.name.trim().to_string();
        let label = if name.is_empty() { id.clone() } else { name.clone() };
        if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!("{label}: the id must be 1–64 characters of a–z, 0–9 and -"));
        }
        if !seen.insert(id.clone()) {
            return Err(format!("{label}: the id \"{id}\" is used twice"));
        }
        if name.is_empty() {
            return Err(format!("{id}: the name is empty"));
        }
        let category = app.category.trim().to_string();
        if !CATEGORIES.contains(&category.as_str()) {
            return Err(format!("{name}: the category must be one of {}", CATEGORIES.join(", ")));
        }
        let matcher = app.matcher.trim().to_string();
        if matcher.chars().count() < 3 {
            return Err(format!("{name}: the match text must be at least 3 characters"));
        }
        let start = trimmed(app.start);
        let flatpak_id = trimmed(app.flatpak_id);
        let service = trimmed(app.service);
        match app.kind {
            AppKind::Process => {}
            AppKind::Flatpak if cfg!(windows) => {
                return Err(format!("{name}: Flatpak apps don't exist on Windows; make it a process or a service"))
            }
            AppKind::Flatpak => match &flatpak_id {
                None => return Err(format!("{name}: a Flatpak app needs its application id")),
                Some(fid) if !valid_flatpak_id(fid) => {
                    return Err(format!("{name}: \"{fid}\" is not a Flatpak application id"))
                }
                Some(_) => {}
            },
            AppKind::Service => match &service {
                None => return Err(format!("{name}: a service needs its {UNIT_NOUN}")),
                Some(unit) if !valid_unit_name(unit) => {
                    return Err(format!(
                        "{name}: \"{unit}\" is not a unit name (letters, digits and @ . _ : - only, not starting with -)"
                    ))
                }
                Some(_) => {}
            },
        }
        out.push(AppDef {
            id,
            name,
            category,
            kind: app.kind,
            matcher,
            start,
            flatpak_id,
            service,
        });
    }
    Ok(out)
}

/// killer's `AUTO_CAT`, checked in the same order: the first category with a
/// keyword that is a whole word of the program's name wins.
const AUTO_CAT: &[(&str, &[&str])] = &[
    ("Browsers", &["chrome", "firefox", "brave", "opera", "vivaldi", "chromium", "epiphany", "midori"]),
    (
        "Communication",
        &[
            "discord", "telegram", "zoom", "teams", "thunderbird", "bluemail", "slack", "signal", "skype",
            "element", "fractal", "nheko", "mail",
        ],
    ),
    (
        "Dev",
        &[
            "code", "zed", "postman", "pycharm", "idea", "clion", "rider", "nvim", "vim", "emacs", "sublime",
            "atom", "kate", "gedit", "dbeaver", "beekeeper",
        ],
    ),
    (
        "Creative",
        &[
            "obs", "gimp", "kdenlive", "inkscape", "blender", "vlc", "mpv", "audacity", "darktable",
            "rawtherapee", "krita", "pitivi", "shotcut", "handbrake",
        ],
    ),
    (
        "Servers",
        &["nginx", "apache", "mysql", "mysqld", "postgres", "redis", "php-fpm", "gunicorn", "uvicorn", "caddy", "mongod"],
    ),
    ("Background", &["dropbox", "nextcloud", "claude", "keybase", "syncthing", "megasync", "insync"]),
];

///
/// killer searched the whole command line, where flags and paths matched the
/// wrong thing: `--enable-chrome-features` filed VS Code under Browsers, and
/// "element" was found inside io.elementary.appcenter. So only the process
/// name and the program's file name count, split into words; a keyword with a
/// dash in it (`php-fpm`) is looked for in the name as it stands.
pub fn auto_category(comm: &str, cmdline: &str) -> &'static str {
    let program = cmdline.split_whitespace().next().unwrap_or("");
    let program = program.rsplit('/').next().unwrap_or(program);
    let name = format!("{comm} {program}").to_lowercase();
    let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let matches = |k: &&str| if k.contains('-') { name.contains(*k) } else { words.contains(k) };
    AUTO_CAT
        .iter()
        .find(|(_, keywords)| keywords.iter().any(matches))
        .map(|(category, _)| *category)
        .unwrap_or("Other")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: &str, kind: AppKind) -> AppDef {
        AppDef {
            id: id.to_string(),
            name: format!("App {id}"),
            category: "Dev".to_string(),
            kind,
            matcher: "something".to_string(),
            start: None,
            flatpak_id: Some("org.example.App".to_string()),
            service: Some("example".to_string()),
        }
    }

    #[test]
    #[cfg(windows)]
    fn the_windows_seed_passes_validation_and_has_no_flatpaks() {
        let seed = seed();
        assert_eq!(validate(seed.clone()).expect("seed must validate"), seed);
        assert!(seed.iter().all(|a| a.kind != AppKind::Flatpak));
        let services: Vec<_> = seed.iter().filter_map(|a| a.service.as_deref()).collect();
        assert_eq!(services, ["Apache2.4", "MySQL80"]);
        assert!(validate(vec![def("a", AppKind::Flatpak)]).unwrap_err().contains("don't exist on Windows"));
    }

    #[test]
    #[cfg(not(windows))]
    fn the_seed_is_killers_list_and_passes_validation() {
        let seed = seed();
        assert_eq!(seed.len(), 24);
        let checked = validate(seed.clone()).expect("seed must validate");
        assert_eq!(checked, seed);
        let services: Vec<_> = seed.iter().filter_map(|a| a.service.as_deref()).collect();
        assert_eq!(services, ["nginx", "apache2", "php8.3-fpm", "mysql"]);
    }

    #[test]
    fn rejects_duplicate_ids_and_bad_slugs() {
        let err = validate(vec![def("a", AppKind::Process), def("a", AppKind::Process)]).unwrap_err();
        assert!(err.contains("used twice"), "{err}");
        for bad in ["", "Chrome", "auto:chrome", "has space", &"x".repeat(65)] {
            assert!(validate(vec![def(bad, AppKind::Process)]).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_unknown_category_and_short_matcher() {
        let mut app = def("a", AppKind::Process);
        app.category = "All".to_string();
        assert!(validate(vec![app]).unwrap_err().contains("category"));

        let mut app = def("a", AppKind::Process);
        app.matcher = " x ".to_string();
        assert!(validate(vec![app]).unwrap_err().contains("at least 3"));
    }

    #[test]
    fn requires_the_field_each_kind_needs() {
        let mut app = def("a", AppKind::Service);
        app.service = None;
        assert!(validate(vec![app]).unwrap_err().contains(UNIT_NOUN));

        let mut app = def("a", AppKind::Service);
        app.service = Some("nginx; reboot".to_string());
        assert!(validate(vec![app]).unwrap_err().contains("not a unit name"));

        // A process app may have no start command: it can still be stopped.
        let mut app = def("a", AppKind::Process);
        app.start = Some("   ".to_string());
        let ok = validate(vec![app]).unwrap();
        assert_eq!(ok[0].start, None);
    }

    #[test]
    #[cfg(not(windows))]
    fn a_flatpak_needs_a_valid_application_id() {
        let mut app = def("a", AppKind::Flatpak);
        app.flatpak_id = Some("  ".to_string());
        assert!(validate(vec![app]).unwrap_err().contains("application id"));

        let mut app = def("a", AppKind::Flatpak);
        app.flatpak_id = Some("--user".to_string());
        assert!(validate(vec![app]).is_err());
    }

    #[test]
    fn tidies_whitespace() {
        let mut app = def("a", AppKind::Process);
        app.name = "  Chrome ".to_string();
        app.matcher = " /opt/google/chrome/chrome ".to_string();
        app.start = Some(" google-chrome ".to_string());
        let ok = validate(vec![app]).unwrap();
        assert_eq!(ok[0].name, "Chrome");
        assert_eq!(ok[0].matcher, "/opt/google/chrome/chrome");
        assert_eq!(ok[0].start.as_deref(), Some("google-chrome"));
    }

    #[test]
    fn json_is_snake_case_with_lowercase_kind_and_nullable_fields() {
        let json = serde_json::to_value(&seed()[0]).unwrap();
        assert_eq!(json["kind"], "process");
        assert_eq!(json["flatpak_id"], serde_json::Value::Null);
        let back: AppDef = serde_json::from_str(
            r#"{"id":"x","name":"X","category":"Dev","kind":"flatpak","matcher":"org.x.X","start":null,"flatpak_id":"org.x.X","service":null}"#,
        )
        .unwrap();
        assert_eq!(back.kind, AppKind::Flatpak);
    }

    #[test]
    fn auto_category_follows_killers_keyword_order() {
        assert_eq!(auto_category("brave", "/opt/brave.com/brave/brave"), "Browsers");
        assert_eq!(auto_category("slack", "/usr/lib/slack/slack"), "Communication");
        assert_eq!(auto_category("code", "/usr/share/code/code"), "Dev");
        assert_eq!(auto_category("blender", "blender"), "Creative");
        assert_eq!(auto_category("redis-server", "redis-server *:6379"), "Servers");
        assert_eq!(auto_category("claude", "claude"), "Background");
        assert_eq!(auto_category("termius-app", "/opt/Termius/termius-app"), "Other");
        // Flags and look-alike words don't count, only the program's own name.
        assert_eq!(auto_category("code", "/usr/share/code/code --enable-chrome-features"), "Dev");
        assert_eq!(auto_category("io.elementary.a", "/usr/bin/io.elementary.appcenter -s"), "Other");
        assert_eq!(auto_category("php-fpm8.3", "php-fpm: master process (/etc/php/8.3/fpm/php-fpm.conf)"), "Servers");
        assert_eq!(auto_category("teams-for-linu", "/app/teams-for-linux/teams-for-linux"), "Communication");
    }
}
