//! Where the caches live on Linux and other Unix systems: the XDG layout
//! under $HOME, with each tool's own dot-directory where it keeps one.

use super::*;

pub(super) fn entries(host: &Host, add: &mut dyn FnMut(Entry)) {
    use Risk::{Danger, Review, Safe};
    let h = &host.home;
    let cache = h.join(".cache");

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
        let active = nvm_active(host, &nvm.join("versions/node"));
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
}
