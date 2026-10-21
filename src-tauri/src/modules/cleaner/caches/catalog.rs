//! The list of caches the tab knows about. Scanning sizes these entries and
//! cleaning re-resolves them by id, so a path never arrives from the frontend.

use super::super::Risk;
use super::procs::Blocker;
use std::fs;
#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
}

impl Host {
    pub fn which(&self, name: &str) -> Option<PathBuf> {
        self.path.iter().map(|d| d.join(name)).find(|p| {
            crate::modules::fs_compat::is_executable(p)
        })
    }

    fn tool(&self, name: &'static str, bins: &[&str], args: &[&'static str]) -> Option<Tool> {
        bins.iter()
            .find_map(|b| self.which(b))
            .map(|bin| Tool { name, bin, args: args.to_vec() })
    }
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

fn version_key(name: &str) -> Vec<u64> {
    name.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The Node version nvm treats as default, by directory name ("v20.11.0").
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

/// The nvm version directory name of the `node` found on PATH, if it lives under nvm.
fn nvm_active(host: &Host, nvm: &Path) -> Option<String> {
    let node = host.which("node")?;
    let node = fs::canonicalize(&node).unwrap_or(node);
    let root = fs::canonicalize(nvm.join("versions/node")).unwrap_or_else(|_| nvm.join("versions/node"));
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
    use Risk::{Danger, Review, Safe};
    let h = &host.home;
    let cache = h.join(".cache");
    let mut out: Vec<Entry> = Vec::new();
    let mut add = |e: Entry| {
        let mut e = e;
        e.paths.retain(|p| real_dir(p));
        if !e.paths.is_empty() {
            out.push(e);
        }
    };

    const JS: &str = "JavaScript";
    const PM: &str = "Package managers";
    const RUST: &str = "Rust";
    const GO: &str = "Go";
    const JVM: &str = "JVM";
    const TEST: &str = "Test tooling";
    const APPS: &str = "Browsers and editors";
    const SMALL: &str = "Small caches";

    // JavaScript
    add(Entry::new("npm", JS, "npm cache", "Downloaded packages; npm fetches them again when needed.", Safe, vec![h.join(".npm/_cacache")])
        .tool(host.tool("npm", &["npm"], &["cache", "clean", "--force"]))
        .blocked_by(&[Blocker::Installing("npm")]));
    add(Entry::new("npm-npx", JS, "npx cache", "Packages run once with npx; fetched again on next use.", Safe, vec![h.join(".npm/_npx")])
        .blocked_by(&[Blocker::Installing("npm")]));
    if let Some(store) = host.pnpm_store.as_ref().filter(|s| s.starts_with(h) && *s != h) {
        add(Entry::new("pnpm-store", JS, "pnpm store", "Prune drops only packages no project links to.", Safe, vec![store.clone()])
            .tool(host.tool("pnpm", &["pnpm"], &["store", "prune"]))
            .blocked_by(&[Blocker::Installing("pnpm")]));
    }
    add(Entry::new("pnpm-cache", JS, "pnpm metadata cache", "Registry metadata; pnpm fetches it again.", Safe, vec![cache.join("pnpm")])
        .blocked_by(&[Blocker::Installing("pnpm")]));
    add(Entry::new("yarn-v1", JS, "yarn cache", "Downloaded packages; yarn fetches them again.", Safe, vec![cache.join("yarn")])
        .tool(host.tool("yarn", &["yarn"], &["cache", "clean"]))
        .blocked_by(&[Blocker::Installing("yarn")]));
    add(Entry::new("yarn-berry", JS, "yarn berry cache", "Global package archives; yarn fetches them again.", Safe, vec![h.join(".yarn/berry/cache")])
        .blocked_by(&[Blocker::Installing("yarn")]));
    add(Entry::new("bun", JS, "bun cache", "Downloaded packages; bun fetches them again.", Safe, vec![h.join(".bun/install/cache")])
        .tool(host.tool("bun", &["bun"], &["pm", "cache", "rm"]))
        .blocked_by(&[Blocker::Installing("bun")]));
    add(Entry::new("nvm-cache", JS, "nvm download cache", "Node archives nvm downloaded; installed versions are unaffected.", Safe, vec![h.join(".nvm/.cache")]));
    let nvm = h.join(".nvm");
    let versions = child_dirs(&nvm.join("versions/node"));
    if let Some(default) = nvm_default(&nvm, &versions) {
        let active = nvm_active(host, &nvm);
        for (name, path) in versions.iter().filter(|(n, _)| *n != default && Some(n) != active.as_ref()) {
            add(Entry::new(&format!("nvm-node-{name}"), JS, &format!("Node {name}"), "An installed Node version other than the default; reinstalling means downloading it again.", Review, vec![path.clone()])
                .action(Action::Remove));
        }
    }

    // Package managers
    add(Entry::new("composer", PM, "composer cache", "Downloaded packages; composer fetches them again.", Safe, vec![cache.join("composer"), h.join(".composer/cache")])
        .tool(host.tool("composer", &["composer"], &["clear-cache"])));
    add(Entry::new("pip", PM, "pip cache", "Downloaded wheels; pip fetches them again.", Safe, vec![cache.join("pip")])
        .tool(host.tool("pip", &["pip", "pip3"], &["cache", "purge"])));
    add(Entry::new("uv", PM, "uv cache", "Downloaded and built packages; uv fetches them again.", Safe, vec![cache.join("uv")])
        .tool(host.tool("uv", &["uv"], &["cache", "clean"])));
    add(Entry::new("poetry", PM, "poetry cache", "Downloaded packages; poetry fetches them again.", Safe, vec![cache.join("pypoetry/cache")]));

    // Rust
    add(Entry::new("cargo-registry", RUST, "cargo registry", "Crate downloads and unpacked sources; re-downloading them is slow.", Review, vec![h.join(".cargo/registry/cache"), h.join(".cargo/registry/src")]));
    add(Entry::new("cargo-git", RUST, "cargo git checkouts", "Git dependencies; cargo clones them again.", Review, vec![h.join(".cargo/git")]));
    let rustup = h.join(".rustup");
    if let Some(default) = rustup_default(&rustup) {
        let mut keep = vec![default];
        keep.extend(host.rustup_active.clone());
        keep.extend(host.rustup_env.clone());
        keep.extend(rustup_overrides(&rustup));
        for (name, path) in child_dirs(&rustup.join("toolchains")).into_iter().filter(|(n, _)| !keep.iter().any(|k| is_default_toolchain(n, k))) {
            add(Entry::new(&format!("rustup-{name}"), RUST, &format!("Rust toolchain {name}"), "A toolchain other than the default; reinstalling it is a large download.", Review, vec![path])
                .action(Action::Remove));
        }
    }

    // Go
    add(Entry::new("go-build", GO, "Go build cache", "Compiled packages; the next build recompiles them.", Safe, vec![cache.join("go-build")])
        .tool(host.tool("go", &["go"], &["clean", "-cache"])));
    add(Entry::new("go-mod", GO, "Go module cache", "Downloaded modules; go fetches them again.", Safe, vec![h.join("go/pkg/mod")])
        .tool(host.tool("go", &["go"], &["clean", "-modcache"])));

    // JVM
    add(Entry::new("gradle", JVM, "Gradle caches", "Dependencies and build outputs; re-downloading them is slow.", Review, vec![h.join(".gradle/caches")])
        .blocked_by(&[Blocker::Gradle]));
    add(Entry::new("m2", JVM, "Maven repository", "Dependencies; re-downloading them is slow, and locally installed artifacts are lost.", Review, vec![h.join(".m2/repository")]));

    // Test tooling
    add(Entry::new("playwright", TEST, "Playwright browsers", "Browser builds for tests; each is a large download.", Review, vec![cache.join("ms-playwright")]));
    add(Entry::new("cypress", TEST, "Cypress binaries", "Cypress app builds; each is a large download.", Review, vec![cache.join("Cypress")]));
    add(Entry::new("puppeteer", TEST, "Puppeteer browsers", "Browser builds for automation; each is a large download.", Review, vec![cache.join("puppeteer")]));

    // Browsers and editors
    add(Entry::new("chrome", APPS, "Chrome cache", "Page and media cache; Chrome rebuilds it as you browse.", Safe, vec![cache.join("google-chrome")])
        .blocked_by(&[Blocker::Comm("Chrome", &["chrome", "google-chrome", "google-chrome-s"])]));
    add(Entry::new("chromium", APPS, "Chromium cache", "Page and media cache; Chromium rebuilds it as you browse.", Safe, vec![cache.join("chromium")])
        .blocked_by(&[Blocker::Comm("Chromium", &["chromium", "chromium-browse", "chrome"])]));
    add(Entry::new("firefox", APPS, "Firefox cache", "Page and media cache; Firefox rebuilds it as you browse.", Safe, vec![cache.join("mozilla/firefox")])
        .blocked_by(&[Blocker::Comm("Firefox", &["firefox", "firefox-bin", "firefox-esr"])]));
    let code = h.join(".config/Code");
    add(Entry::new("vscode", APPS, "VS Code caches", "Compiled-code and extension download caches; VS Code recreates them.", Safe,
        ["Cache", "CachedData", "CachedExtensionVSIXs", "Code Cache", "GPUCache"].iter().map(|d| code.join(d)).collect())
        .blocked_by(&[Blocker::Comm("VS Code", &["code"])]));
    for (name, path) in child_dirs(&cache.join("JetBrains")) {
        if !real_dir(&h.join(".config/JetBrains").join(&name)) {
            add(Entry::new(&format!("jetbrains-{name}"), APPS, &format!("JetBrains {name} caches"), "Caches of an IDE version that is no longer installed.", Review, vec![path])
                .action(Action::Remove));
        }
    }

    // Small caches
    add(Entry::new("thumbnails", SMALL, "Thumbnails", "Image previews; the file manager redraws them.", Safe, vec![cache.join("thumbnails")]));
    add(Entry::new("node-gyp", SMALL, "node-gyp headers", "Node headers for native builds; downloaded again when needed.", Safe, vec![cache.join("node-gyp")]));
    add(Entry::new("electron", SMALL, "Electron downloads", "Electron builds; downloaded again when needed.", Safe, vec![cache.join("electron")]));
    add(Entry::new("typescript", SMALL, "TypeScript cache", "Type-acquisition cache; rebuilt by the editor.", Safe, vec![cache.join("typescript")]));
    add(Entry::new("deno", SMALL, "Deno cache", "Downloaded modules; deno fetches them again.", Safe, vec![cache.join("deno")]));

    // Trash
    add(Entry::new("trash", "Trash", "Trash", "Emptying it deletes these files for good.", Danger, vec![h.join(".local/share/Trash")])
        .tool(host.tool("gio", &["gio"], &["trash", "--empty"])));

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
    fn fake_path(names: &[&str]) -> (PathBuf, Vec<PathBuf>) {
        let dir = scratch("fakepath");
        for n in names {
            let p = dir.join(n);
            fs::write(&p, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir.clone(), vec![dir])
    }

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
