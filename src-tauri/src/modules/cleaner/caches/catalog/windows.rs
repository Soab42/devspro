//! Where the caches live on Windows. Most tools follow the platform convention
//! and keep caches under %LOCALAPPDATA% and settings under %APPDATA%, but the
//! JVM, Rust, Go and bun toolchains keep their Unix dot-directories in the
//! user profile, so those paths match the Linux list.

use super::*;

pub(super) fn entries(host: &Host, add: &mut dyn FnMut(Entry)) {
    use Risk::{Danger, Review, Safe};
    // The joins below are written with "/", which Windows accepts; re-joining
    // the components shows every path with "\" as the user knows it.
    let add = &mut |mut e: Entry| {
        e.paths = e.paths.iter().map(|p| p.components().collect()).collect();
        add(e)
    };
    let h = &host.home;
    let local = &host.local;
    let roaming = &host.roaming;

    // JavaScript
    // npm reports where its cache is; a cache moved outside the profile is not ours to empty.
    let npm = host
        .npm_cache
        .clone()
        .filter(|c| c.starts_with(h) && c != h)
        .unwrap_or_else(|| local.join("npm-cache"));
    add(Entry::new("npm", JS, "npm cache", "Downloaded packages; npm fetches them again when needed.", Safe, vec![npm.join("_cacache")])
        .tool(host.tool("npm", &["npm"], &["cache", "clean", "--force"]))
        .blocked_by(&[Blocker::Installing("npm")]));
    add(Entry::new("npm-npx", JS, "npx cache", "Packages run once with npx; fetched again on next use.", Safe, vec![npm.join("_npx")])
        .blocked_by(&[Blocker::Installing("npm")]));
    if let Some(store) = host.pnpm_store.as_ref().filter(|s| s.starts_with(h) && *s != h) {
        add(Entry::new("pnpm-store", JS, "pnpm store", "Prune drops only packages no project links to.", Safe, vec![store.clone()])
            .tool(host.tool("pnpm", &["pnpm"], &["store", "prune"]))
            .blocked_by(&[Blocker::Installing("pnpm")]));
    }
    add(Entry::new("pnpm-cache", JS, "pnpm metadata cache", "Registry metadata; pnpm fetches it again.", Safe, vec![local.join("pnpm-cache")])
        .blocked_by(&[Blocker::Installing("pnpm")]));
    add(Entry::new("yarn-v1", JS, "yarn cache", "Downloaded packages; yarn fetches them again.", Safe, vec![local.join("Yarn/Cache")])
        .tool(host.tool("yarn", &["yarn"], &["cache", "clean"]))
        .blocked_by(&[Blocker::Installing("yarn")]));
    add(Entry::new("yarn-berry", JS, "yarn berry cache", "Global package archives; yarn fetches them again.", Safe, vec![local.join("Yarn/Berry/cache")])
        .blocked_by(&[Blocker::Installing("yarn")]));
    add(Entry::new("bun", JS, "bun cache", "Downloaded packages; bun fetches them again.", Safe, vec![h.join(".bun/install/cache")])
        .tool(host.tool("bun", &["bun"], &["pm", "cache", "rm"]))
        .blocked_by(&[Blocker::Installing("bun")]));
    for (name, path) in nvm_windows_spare(host) {
        add(Entry::new(&format!("nvm-node-{name}"), JS, &format!("Node {name}"), "An installed Node version other than the one in use; reinstalling means downloading it again.", Review, vec![path])
            .action(Action::Remove));
    }

    // Package managers
    add(Entry::new("composer", PM, "composer cache", "Downloaded packages; composer fetches them again.", Safe, vec![local.join("Composer")])
        .tool(host.tool("composer", &["composer"], &["clear-cache"])));
    add(Entry::new("pip", PM, "pip cache", "Downloaded wheels; pip fetches them again.", Safe, vec![local.join("pip/Cache")])
        .tool(host.tool("pip", &["pip", "pip3"], &["cache", "purge"])));
    add(Entry::new("uv", PM, "uv cache", "Downloaded and built packages; uv fetches them again.", Safe, vec![local.join("uv/cache")])
        .tool(host.tool("uv", &["uv"], &["cache", "clean"])));
    add(Entry::new("poetry", PM, "poetry cache", "Downloaded packages; poetry fetches them again.", Safe, vec![local.join("pypoetry/Cache")]));
    add(Entry::new("nuget-http", PM, "NuGet HTTP cache", "Package downloads and feed metadata; NuGet fetches them again.", Safe, vec![local.join("NuGet/v3-cache"), local.join("NuGet/plugins-cache")])
        .tool(host.tool("dotnet", &["dotnet"], &["nuget", "locals", "http-cache", "--clear"])));
    add(Entry::new("nuget-packages", PM, "NuGet packages", "Every package restored for any project; restoring them again is slow.", Review, vec![h.join(".nuget/packages")])
        .tool(host.tool("dotnet", &["dotnet"], &["nuget", "locals", "global-packages", "--clear"])));

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
    add(Entry::new("go-build", GO, "Go build cache", "Compiled packages; the next build recompiles them.", Safe, vec![local.join("go-build")])
        .tool(host.tool("go", &["go"], &["clean", "-cache"])));
    add(Entry::new("go-mod", GO, "Go module cache", "Downloaded modules; go fetches them again.", Safe, vec![h.join("go/pkg/mod")])
        .tool(host.tool("go", &["go"], &["clean", "-modcache"])));

    // JVM
    add(Entry::new("gradle", JVM, "Gradle caches", "Dependencies and build outputs; re-downloading them is slow.", Review, vec![h.join(".gradle/caches")])
        .blocked_by(&[Blocker::Gradle]));
    add(Entry::new("m2", JVM, "Maven repository", "Dependencies; re-downloading them is slow, and locally installed artifacts are lost.", Review, vec![h.join(".m2/repository")]));

    // Test tooling
    add(Entry::new("playwright", TEST, "Playwright browsers", "Browser builds for tests; each is a large download.", Review, vec![local.join("ms-playwright")]));
    add(Entry::new("cypress", TEST, "Cypress binaries", "Cypress app builds; each is a large download.", Review, vec![local.join("Cypress/Cache")]));
    add(Entry::new("puppeteer", TEST, "Puppeteer browsers", "Browser builds for automation; each is a large download.", Review, vec![h.join(".cache/puppeteer")]));

    // Browsers and editors
    add(Entry::new("chrome", APPS, "Chrome cache", "Page and media cache; Chrome rebuilds it as you browse.", Safe, chromium_caches(&local.join("Google/Chrome/User Data")))
        .blocked_by(&[Blocker::Comm("Chrome", &["chrome"])]));
    add(Entry::new("edge", APPS, "Edge cache", "Page and media cache; Edge rebuilds it as you browse.", Safe, chromium_caches(&local.join("Microsoft/Edge/User Data")))
        .blocked_by(&[Blocker::Comm("Edge", &["msedge"])]));
    add(Entry::new("chromium", APPS, "Chromium cache", "Page and media cache; Chromium rebuilds it as you browse.", Safe, chromium_caches(&local.join("Chromium/User Data")))
        .blocked_by(&[Blocker::Comm("Chromium", &["chrome", "chromium"])]));
    // The Local half of a Firefox profile holds only caches; bookmarks and
    // logins live in the Roaming half, which is never touched.
    add(Entry::new("firefox", APPS, "Firefox cache", "Page and media cache; Firefox rebuilds it as you browse.", Safe, vec![local.join("Mozilla/Firefox/Profiles")])
        .blocked_by(&[Blocker::Comm("Firefox", &["firefox"])]));
    let code = roaming.join("Code");
    add(Entry::new("vscode", APPS, "VS Code caches", "Compiled-code and extension download caches; VS Code recreates them.", Safe,
        ["Cache", "CachedData", "CachedExtensionVSIXs", "Code Cache", "GPUCache"].iter().map(|d| code.join(d)).collect())
        .blocked_by(&[Blocker::Comm("VS Code", &["code"])]));
    // %LOCALAPPDATA%\JetBrains also holds Toolbox, which is where Toolbox
    // installs the IDEs themselves, so only versioned IDE folders are considered.
    for (name, path) in child_dirs(&local.join("JetBrains")).into_iter().filter(|(n, _)| is_ide_version(n)) {
        if !real_dir(&roaming.join("JetBrains").join(&name)) {
            add(Entry::new(&format!("jetbrains-{name}"), APPS, &format!("JetBrains {name} caches"), "Caches of an IDE version that is no longer installed.", Review, vec![path])
                .action(Action::Remove));
        }
    }

    // Small caches
    add(Entry::new("node-gyp", SMALL, "node-gyp headers", "Node headers for native builds; downloaded again when needed.", Safe, vec![local.join("node-gyp/Cache")]));
    add(Entry::new("electron", SMALL, "Electron downloads", "Electron builds; downloaded again when needed.", Safe, vec![local.join("electron/Cache")]));
    add(Entry::new("typescript", SMALL, "TypeScript cache", "Type-acquisition cache; rebuilt by the editor.", Safe, vec![local.join("Microsoft/TypeScript")]));
    add(Entry::new("deno", SMALL, "Deno cache", "Downloaded modules; deno fetches them again.", Safe, vec![local.join("deno")]));
    add(Entry::new("crash-dumps", SMALL, "Crash dumps", "Memory dumps Windows saved when programs crashed.", Safe, vec![local.join("CrashDumps")]));
    if let Some(temp) = host.temp.as_ref().filter(|t| t.starts_with(h) && *t != h) {
        add(Entry::new("temp", SMALL, "Temporary files", "Files programs left in the temp folder; anything still in use is skipped.", Review, vec![temp.clone()])
            .action(Action::Sweep));
    }

    // Trash. An empty bin still holds its desktop.ini, so only bins with a
    // deleted item in them ($I… records its origin, $R… is the item) count.
    let bins = host.recycle_bins.iter().filter(|b| has_deleted_items(b)).cloned().collect();
    add(Entry::new("trash", "Trash", "Recycle Bin", "Emptying it deletes these files for good.", Danger, bins)
        .action(Action::RecycleBin));
}

fn has_deleted_items(bin: &Path) -> bool {
    fs::read_dir(bin)
        .map(|mut read| read.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("$R"))))
        .unwrap_or(false)
}

/// The disk caches of every profile under a Chromium-family `User Data`
/// folder. Never the folder itself: it holds history, passwords and cookies.
fn chromium_caches(user_data: &Path) -> Vec<PathBuf> {
    child_dirs(user_data)
        .into_iter()
        .filter(|(_, profile)| real_file(&profile.join("Preferences")))
        .flat_map(|(_, profile)| ["Cache", "Code Cache", "GPUCache"].map(|d| profile.join(d)))
        .collect()
}

/// "IntelliJIdea2024.2", "PhpStorm2023.3": a product name followed by a version.
fn is_ide_version(name: &str) -> bool {
    let Some(start) = name.find(|c: char| c.is_ascii_digit()) else { return false };
    start > 0 && name[start..].contains('.') && name[start..].chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// nvm-windows: installed versions other than the one in use, as (name, path).
/// nvm-windows has no default alias; the version in use is the one its
/// symlink points at, or else the one holding the `node` on PATH. When neither
/// can be told, nothing is offered, since there is no safe choice.
fn nvm_windows_spare(host: &Host) -> Vec<(String, PathBuf)> {
    let settings = fs::read_to_string(host.roaming.join("nvm/settings.txt")).unwrap_or_default();
    let setting = |key: &str| {
        settings.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == key).then(|| PathBuf::from(v.trim())).filter(|p| p.is_absolute())
        })
    };
    let root = setting("root").unwrap_or_else(|| host.roaming.join("nvm"));
    let versions: Vec<_> = child_dirs(&root).into_iter().filter(|(n, _)| n.starts_with('v') && n[1..].starts_with(|c: char| c.is_ascii_digit())).collect();
    let mut active: Vec<String> = Vec::new();
    if let Some(link) = setting("path").and_then(|p| fs::read_link(p).ok()) {
        active.extend(link.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    active.extend(nvm_active(host, &root));
    if active.is_empty() {
        return Vec::new();
    }
    versions.into_iter().filter(|(n, _)| !active.contains(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-caches-win-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// A profile laid out like a real one: Local and Roaming under AppData.
    fn host(home: &Path) -> Host {
        Host {
            home: home.to_path_buf(),
            local: home.join("AppData/Local"),
            roaming: home.join("AppData/Roaming"),
            ..Host::default()
        }
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
    fn finds_caches_in_their_windows_homes() {
        let home = scratch("detect");
        assert!(ids(&host(&home)).is_empty());
        for rel in [
            "AppData/Local/npm-cache/_cacache",
            "AppData/Local/Yarn/Cache",
            "AppData/Local/pip/Cache",
            "AppData/Local/Composer",
            "AppData/Local/NuGet/v3-cache",
            ".nuget/packages",
            ".cargo/registry/src",
            "AppData/Local/go-build",
            "go/pkg/mod",
            ".gradle/caches",
            ".m2/repository",
            "AppData/Local/ms-playwright",
            "AppData/Roaming/Code/CachedData",
            "AppData/Local/Mozilla/Firefox/Profiles",
            "AppData/Local/CrashDumps",
        ] {
            mk(&home, rel);
        }
        let got = ids(&host(&home));
        assert_eq!(
            got,
            vec!["npm", "yarn-v1", "composer", "pip", "nuget-http", "nuget-packages", "cargo-registry", "go-build", "go-mod", "gradle", "m2", "playwright", "firefox", "vscode", "crash-dumps"]
        );
        let h = host(&home);
        assert_eq!(find(&h, "pip").paths, vec![home.join("AppData/Local").join("pip/Cache")]);
        assert_eq!(find(&h, "nuget-packages").risk, Risk::Review);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn unset_known_folders_never_resolve_against_the_working_directory() {
        let home = scratch("unset");
        // A relative "pip/Cache" next to the test binary must not be picked up.
        let h = Host { home: home.clone(), ..Host::default() };
        assert!(catalog(&h).iter().all(|e| e.paths.iter().all(|p| p.is_absolute())));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn npm_uses_the_cache_npm_reports_but_only_inside_the_profile() {
        let home = scratch("npm");
        let moved = mk(&home, "caches/npm/_cacache");
        mk(&home, "AppData/Local/npm-cache/_cacache");
        let mut h = host(&home);
        h.npm_cache = Some(home.join("caches/npm"));
        assert_eq!(find(&h, "npm").paths, vec![moved]);
        let outside = scratch("npm-outside");
        h.npm_cache = Some(outside.clone());
        assert_eq!(find(&h, "npm").paths, vec![home.join("AppData/Local/npm-cache").join("_cacache")]);
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn chromium_browsers_offer_profile_caches_never_user_data() {
        let home = scratch("chrome");
        let data = home.join("AppData/Local/Google/Chrome/User Data");
        for p in ["Default", "Profile 1"] {
            mk(&data, &format!("{p}/Cache/Cache_Data"));
            mk(&data, &format!("{p}/Code Cache"));
            fs::write(data.join(p).join("Preferences"), "{}").unwrap();
        }
        // Not a profile: no Preferences file.
        mk(&data, "ShaderCache/Cache");
        let e = find(&host(&home), "chrome");
        assert_eq!(
            e.paths,
            vec![data.join("Default/Cache"), data.join("Default/Code Cache"), data.join("Profile 1/Cache"), data.join("Profile 1/Code Cache")]
        );
        assert!(!e.paths.contains(&data));
        let procs = [super::super::super::procs::Proc { comm: "chrome".into(), args: vec![] }];
        assert_eq!(e.blockers[0].hit(&procs).as_deref(), Some("Chrome is running"));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn jetbrains_skips_toolbox_and_installed_versions() {
        let home = scratch("jb");
        mk(&home, "AppData/Local/JetBrains/Toolbox/apps");
        mk(&home, "AppData/Local/JetBrains/PhpStorm2023.1");
        mk(&home, "AppData/Local/JetBrains/PhpStorm2024.2");
        mk(&home, "AppData/Roaming/JetBrains/PhpStorm2024.2");
        assert_eq!(ids(&host(&home)), vec!["jetbrains-PhpStorm2023.1"]);
        assert!(is_ide_version("IntelliJIdea2024.2") && !is_ide_version("Toolbox") && !is_ide_version("2024.2"));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn nvm_windows_keeps_the_version_on_path() {
        let home = scratch("nvm");
        for v in ["v18.1.0", "v20.11.0", "v22.0.0"] {
            mk(&home, &format!("AppData/Roaming/nvm/{v}"));
        }
        let mut h = host(&home);
        // Nothing tells which version is in use: offer none.
        assert!(ids(&h).is_empty());
        let active = home.join("AppData/Roaming/nvm/v20.11.0");
        fs::write(active.join("node.exe"), "").unwrap();
        h.path = vec![active];
        assert_eq!(ids(&h), vec!["nvm-node-v18.1.0", "nvm-node-v22.0.0"]);
        assert_eq!(find(&h, "nvm-node-v18.1.0").action, Action::Remove);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn nvm_windows_reads_root_from_its_settings() {
        let home = scratch("nvm-root");
        let root = mk(&home, "nvm4w");
        mk(&root, "v18.1.0");
        mk(&root, "v20.0.0");
        fs::write(root.join("v20.0.0/node.exe"), "").unwrap();
        mk(&home, "AppData/Roaming/nvm");
        fs::write(home.join("AppData/Roaming/nvm/settings.txt"), format!("root: {}\r\npath: C:\\nowhere\r\n", root.display())).unwrap();
        let mut h = host(&home);
        h.path = vec![root.join("v20.0.0")];
        assert_eq!(ids(&h), vec!["nvm-node-v18.1.0"]);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn temp_is_swept_and_the_recycle_bin_emptied_through_the_shell() {
        let home = scratch("temp");
        let temp = mk(&home, "AppData/Local/Temp");
        let bin = mk(&home, "bin/S-1-5-21-1");
        fs::write(bin.join("desktop.ini"), "x").unwrap();
        let mut h = host(&home);
        h.temp = Some(temp.clone());
        h.recycle_bins = vec![bin.clone()];
        assert!(catalog(&h).iter().all(|e| e.id != "trash"), "a bin with only desktop.ini is empty");
        fs::write(bin.join("$RAB12CD.txt"), "deleted").unwrap();
        let t = find(&h, "temp");
        assert_eq!((t.action, t.risk, t.paths), (Action::Sweep, Risk::Review, vec![temp]));
        let r = find(&h, "trash");
        assert_eq!((r.action, r.risk, r.paths.clone()), (Action::RecycleBin, Risk::Danger, vec![bin]));
        assert_eq!(r.method(), "empty Recycle Bin");
        // A temp folder outside the profile (C:\Windows\Temp) is never offered.
        h.temp = Some(PathBuf::from("C:\\Windows\\Temp"));
        assert!(catalog(&h).iter().all(|e| e.id != "temp"));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn tools_resolve_through_their_windows_extensions() {
        let home = scratch("argv");
        mk(&home, "AppData/Local/npm-cache/_cacache");
        mk(&home, "AppData/Local/NuGet/v3-cache");
        let bin = mk(&home, "nodejs");
        // The extensionless `npm` is Git Bash's shell script and must be passed over.
        fs::write(bin.join("npm"), "#!/bin/sh\n").unwrap();
        fs::write(bin.join("npm.cmd"), "@echo off\n").unwrap();
        fs::write(bin.join("dotnet.exe"), "").unwrap();
        let mut h = host(&home);
        h.path = vec![bin.clone()];
        let npm = find(&h, "npm").tool.unwrap();
        assert_eq!((npm.bin, npm.args), (bin.join("npm.cmd"), vec!["cache", "clean", "--force"]));
        assert_eq!(find(&h, "nuget-http").method(), "dotnet nuget locals http-cache --clear");
        fs::remove_dir_all(&home).unwrap();
    }
}
