//! The list of caches the tab knows about. Scanning sizes these entries and
//! cleaning re-resolves them by id, so a path never arrives from the frontend.

use super::super::Risk;
use super::procs::Blocker;
use std::fs;
#[cfg(all(test, unix))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// Where each cache lives differs per OS; everything else in this file is shared.
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

const JS: &str = "JavaScript";
const PM: &str = "Package managers";
const RUST: &str = "Rust";
const GO: &str = "Go";
const JVM: &str = "JVM";
const TEST: &str = "Test tooling";
const APPS: &str = "Browsers and editors";
const SMALL: &str = "Small caches";

/// A Laravel log is only worth offering once it is this big.
pub const LOG_MIN_BYTES: u64 = 50 * 1024 * 1024;

/// Safe findings at least this big are pre-ticked.
pub const SUGGEST_MIN_BYTES: u64 = 100 * 1024 * 1024;

/// What cleaning does to an entry's paths when no tool command applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Remove what is inside each path, keep the path.
    Empty,
    /// Remove each path itself (a Node version, a toolchain).
    Remove,
    /// Cut each file to 0 bytes.
    Truncate,
    /// Like `Empty`, but an item that cannot be removed (a temp file a running
    /// program still holds open) is skipped rather than failing the entry.
    #[cfg(windows)]
    Sweep,
    /// Empty the Recycle Bin through the shell, which owns its bookkeeping.
    #[cfg(windows)]
    RecycleBin,
}

/// The tool's own clean command: a resolved binary and a fixed argv, never a shell line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: &'static str,
    pub bin: PathBuf,
    pub args: Vec<&'static str>,
}

impl Tool {
    pub fn display(&self) -> String {
        format!("{} {}", self.name, self.args.join(" "))
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub group: &'static str,
    pub label: String,
    pub reason: &'static str,
    pub risk: Risk,
    pub paths: Vec<PathBuf>,
    pub action: Action,
    pub tool: Option<Tool>,
    pub blockers: Vec<Blocker>,
}

impl Entry {
    fn new(id: &str, group: &'static str, label: &str, reason: &'static str, risk: Risk, paths: Vec<PathBuf>) -> Entry {
        Entry {
            id: id.to_string(),
            group,
            label: label.to_string(),
            reason,
            risk,
            paths,
            action: Action::Empty,
            tool: None,
            blockers: Vec::new(),
        }
    }

    fn action(mut self, action: Action) -> Entry {
        self.action = action;
        self
    }

    fn tool(mut self, tool: Option<Tool>) -> Entry {
        self.tool = tool;
        self
    }

    fn blocked_by(mut self, blockers: &[Blocker]) -> Entry {
        self.blockers = blockers.to_vec();
        self
    }

    /// What the table's "method" badge says.
    pub fn method(&self) -> String {
        match (&self.tool, self.action) {
            (Some(t), _) => t.display(),
            (None, Action::Truncate) => "truncate".to_string(),
            #[cfg(windows)]
            (None, Action::RecycleBin) => "empty Recycle Bin".to_string(),
            (None, _) => "delete".to_string(),
        }
    }

    /// Where free space is read: the nearest ancestor of the first path that
    /// exists right now, since the path itself may be gone after cleaning.
    pub fn probe(&self) -> PathBuf {
        let mut dir = self.paths.first().and_then(|p| p.parent()).unwrap_or(Path::new("/"));
        while !dir.exists() {
            match dir.parent() {
                Some(up) => dir = up,
                None => break,
            }
        }
        dir.to_path_buf()
    }
}

/// Everything the catalog needs from the machine, so tests can supply their own.
#[derive(Debug, Clone, Default)]
pub struct Host {
    pub home: PathBuf,
    /// The directories of $PATH.
    pub path: Vec<PathBuf>,
    /// Output of `pnpm store path`, when pnpm is installed.
    pub pnpm_store: Option<PathBuf>,
    /// Project folders of the Laravel sites DevsPro manages.
    pub laravel_sites: Vec<PathBuf>,
    /// First word of `rustup show active-toolchain`.
    pub rustup_active: Option<String>,
    /// $RUSTUP_TOOLCHAIN.
    pub rustup_env: Option<String>,
    /// %LOCALAPPDATA%, where most tools keep their caches on Windows.
    #[cfg(windows)]
    pub local: PathBuf,
    /// %APPDATA% (Roaming): VS Code, nvm-windows and JetBrains settings.
    #[cfg(windows)]
    pub roaming: PathBuf,
    /// %TEMP%.
    #[cfg(windows)]
    pub temp: Option<PathBuf>,
    /// Output of `npm config get cache`, when npm is installed.
    #[cfg(windows)]
    pub npm_cache: Option<PathBuf>,
    /// This user's `$Recycle.Bin\<SID>` folder on each fixed drive.
    #[cfg(windows)]
    pub recycle_bins: Vec<PathBuf>,
}

impl Host {
    pub fn which(&self, name: &str) -> Option<PathBuf> {
        self.path.iter().flat_map(|d| candidates(d, name)).find(|p| crate::modules::fs_compat::is_executable(p))
    }

    fn tool(&self, name: &'static str, bins: &[&str], args: &[&'static str]) -> Option<Tool> {
        bins.iter()
            .find_map(|b| self.which(b))
            .map(|bin| Tool { name, bin, args: args.to_vec() })
    }
}

#[cfg(unix)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

/// Windows runs `npm` as `npm.cmd`; the bare `npm` beside it is a shell script
/// for Git Bash that `CreateProcess` refuses, so only real extensions count.
#[cfg(windows)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    ["exe", "cmd", "bat", "com"].iter().map(|ext| dir.join(format!("{name}.{ext}"))).collect()
}

/// A real directory, not a symlink to one: emptying through a link could reach anywhere.
fn real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

fn real_file(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.is_file()).unwrap_or(false)
}

fn child_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<_> = read
        .flatten()
        .filter(|e| real_dir(&e.path()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort();
    out
}

#[cfg(unix)]
fn version_key(name: &str) -> Vec<u64> {
    name.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The Node version nvm treats as default, by directory name ("v20.11.0").
#[cfg(unix)]
fn nvm_default(nvm: &Path, versions: &[(String, PathBuf)]) -> Option<String> {
    let highest = || versions.iter().map(|(n, _)| n).max_by_key(|n| version_key(n)).cloned();
    let mut alias = fs::read_to_string(nvm.join("alias/default")).ok()?.trim().to_string();
    // An alias can point at another alias ("lts/iron" -> "v20.11.0").
    for _ in 0..3 {
        if alias.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()) {
            let want = alias.trim_start_matches('v').to_string();
            return versions
                .iter()
                .map(|(n, _)| n)
                .filter(|n| {
                    let bare = n.trim_start_matches('v');
                    bare == want || bare.starts_with(&format!("{want}."))
                })
                .max_by_key(|n| version_key(n))
                .cloned();
        }
        if matches!(alias.as_str(), "node" | "stable") {
            return highest();
        }
        match fs::read_to_string(nvm.join("alias").join(&alias)) {
            Ok(next) if !alias.contains("..") => alias = next.trim().to_string(),
            _ => return None,
        }
    }
    None
}

/// The name of the version directory under `versions` that holds the `node`
/// found on PATH, if it lives there.
fn nvm_active(host: &Host, versions: &Path) -> Option<String> {
    let node = host.which("node")?;
    let node = fs::canonicalize(&node).unwrap_or(node);
    let root = fs::canonicalize(versions).unwrap_or_else(|_| versions.to_path_buf());
    let rel = node.strip_prefix(&root).ok()?;
    rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned())
}

/// Toolchains named in `[overrides]` of rustup's settings file.
fn rustup_overrides(rustup: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(rustup.join("settings.toml")) else { return Vec::new() };
    let mut in_overrides = false;
    let mut out = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_overrides = line == "[overrides]";
        } else if in_overrides {
            if let Some((_, v)) = line.rsplit_once('=') {
                out.push(v.trim().trim_matches('"').to_string());
            }
        }
    }
    out
}

/// The toolchain rustup treats as default, from its settings file.
fn rustup_default(rustup: &Path) -> Option<String> {
    let text = fs::read_to_string(rustup.join("settings.toml")).ok()?;
    text.lines().find_map(|l| {
        let (key, value) = l.split_once('=')?;
        (key.trim() == "default_toolchain").then(|| value.trim().trim_matches('"').to_string())
    })
}

fn is_default_toolchain(dir_name: &str, default: &str) -> bool {
    dir_name == default || dir_name.starts_with(&format!("{default}-"))
}

/// Every cache that exists on this machine, unsized and in display order.
pub fn catalog(host: &Host) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut add = |e: Entry| {
        let mut e = e;
        // Relative paths come only from an unset base folder (a Windows
        // known folder missing from the environment); never resolve them.
        e.paths.retain(|p| p.is_absolute() && real_dir(p));
        if !e.paths.is_empty() {
            out.push(e);
        }
    };
    platform::entries(host, &mut add);
    out.extend(laravel_logs(host));
    out
}

/// One entry per oversized `storage/logs/*.log` in a managed Laravel project.
fn laravel_logs(host: &Host) -> Vec<Entry> {
    let mut out = Vec::new();
    for project in &host.laravel_sites {
        let dir = project.join("storage/logs");
        if !real_dir(&dir) {
            continue;
        }
        let Ok(read) = fs::read_dir(&dir) else { continue };
        let mut logs: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "log") && real_file(p))
            .filter(|p| fs::symlink_metadata(p).map(|m| m.len() > LOG_MIN_BYTES).unwrap_or(false))
            .collect();
        logs.sort();
        for log in logs {
            let name = log.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let site = project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(
                Entry::new(&format!("laravel-log:{}", log.display()), "Laravel logs", &format!("{site}: {name}"), "An oversized log; cleaning empties the file and keeps it in place.", Risk::Review, vec![log])
                    .action(Action::Truncate),
            );
        }
    }
    out
}

/// The folder a path of this entry is cleaned within.
pub fn root_for(entry: &Entry, path: &Path) -> PathBuf {
    match entry.action {
        Action::Empty => path.to_path_buf(),
        #[cfg(windows)]
        Action::Sweep | Action::RecycleBin => path.to_path_buf(),
        Action::Remove | Action::Truncate => path.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

/// True when the entry should be pre-ticked.
pub fn suggested(risk: Risk, bytes: u64) -> bool {
    risk == Risk::Safe && bytes >= SUGGEST_MIN_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-caches-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn host(home: &Path) -> Host {
        Host { home: home.to_path_buf(), ..Host::default() }
    }

    fn mk(home: &Path, rel: &str) -> PathBuf {
        let p = home.join(rel);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn ids(h: &Host) -> Vec<String> {
        catalog(h).into_iter().map(|e| e.id).collect()
    }

    fn find(h: &Host, id: &str) -> Entry {
        catalog(h).into_iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no entry {id}"))
    }

    #[cfg(unix)]
    #[test]
    fn detects_only_what_exists() {
        let home = scratch("detect");
        assert!(ids(&host(&home)).is_empty());
        mk(&home, ".npm/_cacache");
        mk(&home, ".cache/pip");
        mk(&home, ".cargo/registry/src");
        let got = ids(&host(&home));
        assert_eq!(got, vec!["npm", "pip", "cargo-registry"]);
        // Only the existing half of a two-path entry is kept.
        assert_eq!(find(&host(&home), "cargo-registry").paths, vec![home.join(".cargo/registry/src")]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_cache_dir_is_not_offered() {
        let home = scratch("symlink");
        let elsewhere = scratch("symlink-target");
        fs::create_dir_all(home.join(".cache")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, home.join(".cache/pip")).unwrap();
        assert!(ids(&host(&home)).is_empty());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn risk_levels_follow_the_rules() {
        let home = scratch("risk");
        for rel in [".cache/pip", ".gradle/caches", ".m2/repository", ".cargo/registry/cache", ".cache/ms-playwright", ".cache/Cypress", ".cache/puppeteer", ".local/share/Trash", ".cache/thumbnails"] {
            mk(&home, rel);
        }
        let h = host(&home);
        assert_eq!(find(&h, "pip").risk, Risk::Safe);
        assert_eq!(find(&h, "thumbnails").risk, Risk::Safe);
        for id in ["gradle", "m2", "cargo-registry", "playwright", "cypress", "puppeteer"] {
            assert_eq!(find(&h, id).risk, Risk::Review, "{id}");
        }
        assert_eq!(find(&h, "trash").risk, Risk::Danger);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn suggested_means_safe_and_at_least_100_mib() {
        let mib = 1024 * 1024;
        assert!(suggested(Risk::Safe, 100 * mib));
        assert!(!suggested(Risk::Safe, 100 * mib - 1));
        assert!(!suggested(Risk::Review, 10_000 * mib));
        assert!(!suggested(Risk::Danger, 10_000 * mib));
    }

    #[cfg(unix)]
    #[test]
    fn nvm_keeps_the_default_version_and_offers_the_rest() {
        let home = scratch("nvm");
        for v in ["v18.1.0", "v20.11.0", "v20.9.0", "v22.0.0"] {
            mk(&home, &format!(".nvm/versions/node/{v}"));
        }
        mk(&home, ".nvm/alias");
        // Without a default alias nothing is offered: there is no safe choice.
        assert!(ids(&host(&home)).is_empty());
        fs::write(home.join(".nvm/alias/default"), "20\n").unwrap();
        let got = ids(&host(&home));
        assert_eq!(got, vec!["nvm-node-v18.1.0", "nvm-node-v20.9.0", "nvm-node-v22.0.0"]);
        assert!(got.iter().all(|i| i != "nvm-node-v20.11.0"));
        let e = find(&host(&home), "nvm-node-v22.0.0");
        assert_eq!((e.risk, e.action), (Risk::Review, Action::Remove));
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nvm_follows_an_alias_to_another_alias() {
        let home = scratch("nvm-alias");
        mk(&home, ".nvm/versions/node/v18.1.0");
        mk(&home, ".nvm/versions/node/v20.11.0");
        mk(&home, ".nvm/alias/lts");
        fs::write(home.join(".nvm/alias/default"), "lts/iron").unwrap();
        fs::write(home.join(".nvm/alias/lts/iron"), "v20.11.0").unwrap();
        assert_eq!(ids(&host(&home)), vec!["nvm-node-v18.1.0"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rustup_keeps_the_default_toolchain() {
        let home = scratch("rustup");
        mk(&home, ".rustup/toolchains/stable-x86_64-unknown-linux-gnu");
        mk(&home, ".rustup/toolchains/nightly-x86_64-unknown-linux-gnu");
        assert!(ids(&host(&home)).is_empty());
        fs::write(home.join(".rustup/settings.toml"), "version = \"12\"\ndefault_toolchain = \"stable-x86_64-unknown-linux-gnu\"\n").unwrap();
        assert_eq!(ids(&host(&home)), vec!["rustup-nightly-x86_64-unknown-linux-gnu"]);
        fs::write(home.join(".rustup/settings.toml"), "default_toolchain = \"stable\"\n").unwrap();
        assert_eq!(ids(&host(&home)), vec!["rustup-nightly-x86_64-unknown-linux-gnu"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn jetbrains_offers_only_versions_without_a_config_dir() {
        let home = scratch("jb");
        mk(&home, ".cache/JetBrains/IntelliJIdea2023.1");
        mk(&home, ".cache/JetBrains/IntelliJIdea2024.2");
        mk(&home, ".config/JetBrains/IntelliJIdea2024.2");
        assert_eq!(ids(&host(&home)), vec!["jetbrains-IntelliJIdea2023.1"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn pnpm_store_must_live_under_home() {
        let home = scratch("pnpm");
        let store = mk(&home, ".local/share/pnpm/store");
        let mut h = host(&home);
        h.pnpm_store = Some(store.clone());
        assert_eq!(ids(&h), vec!["pnpm-store"]);
        let outside = scratch("pnpm-outside");
        h.pnpm_store = Some(outside.clone());
        assert!(ids(&h).is_empty());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn laravel_logs_over_50_mib_only() {
        let home = scratch("laravel");
        let project = mk(&home, "sites/shop");
        let logs = mk(&project, "storage/logs");
        let big = fs::File::create(logs.join("laravel.log")).unwrap();
        big.set_len(LOG_MIN_BYTES + 1).unwrap();
        let small = fs::File::create(logs.join("small.log")).unwrap();
        small.set_len(LOG_MIN_BYTES).unwrap();
        fs::File::create(logs.join("notes.txt")).unwrap().write_all(b"x").unwrap();
        let mut h = host(&home);
        h.laravel_sites = vec![project.clone()];
        let found = catalog(&h);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].paths, vec![logs.join("laravel.log")]);
        assert_eq!(found[0].action, Action::Truncate);
        assert_eq!(found[0].label, "shop: laravel.log");
        fs::remove_dir_all(&home).unwrap();
    }

    /// A PATH directory holding an executable for each name.
    #[cfg(unix)]
    fn fake_path(names: &[&str]) -> (PathBuf, Vec<PathBuf>) {
        let dir = scratch("fakepath");
        for n in names {
            let p = dir.join(n);
            fs::write(&p, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir.clone(), vec![dir])
    }

    #[cfg(unix)]
    #[test]
    fn each_tool_uses_its_fixed_argv() {
        let home = scratch("argv");
        for rel in [".npm/_cacache", ".cache/pnpm", ".cache/yarn", ".bun/install/cache", ".cache/composer", ".cache/pip", ".cache/uv", ".cache/go-build", "go/pkg/mod", ".local/share/Trash", ".local/share/pnpm/store"] {
            mk(&home, rel);
        }
        let (fp, path) = fake_path(&["npm", "pnpm", "yarn", "bun", "composer", "pip", "uv", "go", "gio"]);
        let mut h = host(&home);
        h.path = path;
        h.pnpm_store = Some(home.join(".local/share/pnpm/store"));
        let argv = |id: &str| {
            let t = find(&h, id).tool.unwrap_or_else(|| panic!("{id} has no tool"));
            (t.name, t.args)
        };
        assert_eq!(argv("npm"), ("npm", vec!["cache", "clean", "--force"]));
        assert_eq!(argv("pnpm-store"), ("pnpm", vec!["store", "prune"]));
        assert_eq!(argv("yarn-v1"), ("yarn", vec!["cache", "clean"]));
        assert_eq!(argv("bun"), ("bun", vec!["pm", "cache", "rm"]));
        assert_eq!(argv("composer"), ("composer", vec!["clear-cache"]));
        assert_eq!(argv("pip"), ("pip", vec!["cache", "purge"]));
        assert_eq!(argv("uv"), ("uv", vec!["cache", "clean"]));
        assert_eq!(argv("go-build"), ("go", vec!["clean", "-cache"]));
        assert_eq!(argv("go-mod"), ("go", vec!["clean", "-modcache"]));
        assert_eq!(argv("trash"), ("gio", vec!["trash", "--empty"]));
        assert_eq!(find(&h, "npm").method(), "npm cache clean --force");
        // No tool on PATH: the directory is emptied instead.
        h.path = Vec::new();
        assert!(find(&h, "npm").tool.is_none());
        assert_eq!(find(&h, "npm").method(), "delete");
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&fp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nvm_never_offers_the_version_on_path() {
        let home = scratch("nvm-active");
        mk(&home, ".nvm/versions/node/v18.1.0");
        mk(&home, ".nvm/versions/node/v16.0.0");
        mk(&home, ".nvm/versions/node/v20.0.0");
        mk(&home, ".nvm/alias");
        fs::write(home.join(".nvm/alias/default"), "20").unwrap();
        let bin = mk(&home, ".nvm/versions/node/v18.1.0/bin");
        let node = bin.join("node");
        fs::write(&node, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        let mut h = host(&home);
        assert_eq!(ids(&h), vec!["nvm-node-v16.0.0", "nvm-node-v18.1.0"]);
        h.path = vec![bin];
        assert_eq!(ids(&h), vec!["nvm-node-v16.0.0"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rustup_never_offers_active_env_or_override_toolchains() {
        let home = scratch("rustup-keep");
        for t in ["stable-x", "nightly-x", "beta-x", "1.70.0-x", "1.60.0-x", "old-x"] {
            mk(&home, &format!(".rustup/toolchains/{t}"));
        }
        fs::write(
            home.join(".rustup/settings.toml"),
            "default_toolchain = \"stable-x\"\n[overrides]\n\"/work/p\" = \"1.70.0-x\"\n[other]\nx = \"old-x\"\n",
        )
        .unwrap();
        let mut h = host(&home);
        assert_eq!(ids(&h), vec!["rustup-1.60.0-x", "rustup-beta-x", "rustup-nightly-x", "rustup-old-x"]);
        h.rustup_active = Some("nightly-x".into());
        h.rustup_env = Some("beta".into());
        assert_eq!(ids(&h), vec!["rustup-1.60.0-x", "rustup-old-x"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn probe_is_the_nearest_surviving_ancestor() {
        let home = scratch("probe");
        let e = Entry::new("x", "g", "x", "r", Risk::Safe, vec![home.join("a/b/c")]);
        assert_eq!(e.probe(), home);
        mk(&home, "a");
        assert_eq!(e.probe(), home.join("a"));
        fs::remove_dir_all(&home).unwrap();
    }
}
