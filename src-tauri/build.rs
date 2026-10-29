fn main() {
    // Must run before `tauri_build::build()`: declaring `bundle.externalBin` in
    // tauri.conf.json makes that call hard-require `binaries/ldm-helper-<target-triple>`
    // to exist already, for every build (debug included), not just packaged ones.
    prepare_helper_sidecar();
    tauri_build::build();
}

/// Populates `binaries/ldm-helper-<target-triple>`, the exact name/location
/// `bundle.externalBin` expects. Preferred path: copy over the real `ldm-helper` binary
/// already built into `target/<profile>/` by a prior `cargo build --workspace` (or
/// `-p ldm-helper`) — that has to be a separate, prior step (documented, not automated
/// here) because shelling out to `cargo` recursively from inside a build script risks
/// deadlocking on Cargo's own package-cache lock, especially under concurrent builds.
///
/// If the real binary isn't there yet (e.g. a first-ever `cargo build` before that
/// workspace step has run), a placeholder script is dropped in its place instead of
/// failing the build outright — `tauri_build::build()`'s existence check is satisfied
/// either way, and running the placeholder gives a clear message rather than "exec
/// format error" if someone skips the prerequisite and it ends up packaged for real.
fn prepare_helper_sidecar() {
    // Read when the script runs, not baked in with env!: checkouts sharing one
    // CARGO_TARGET_DIR (worktrees) share this compiled script too, and a baked-in
    // path would copy the helper into whichever checkout built it first.
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let target_triple = std::env::var("TARGET").unwrap_or_default();
    if target_triple.is_empty() {
        return;
    }
    let is_windows = target_triple.contains("windows");

    let binaries_dir = manifest_dir.join("binaries");
    if let Err(e) = std::fs::create_dir_all(&binaries_dir) {
        println!("cargo:warning=failed to create {binaries_dir:?}: {e}");
        return;
    }

    let dest_name = if is_windows {
        format!("ldm-helper-{target_triple}.exe")
    } else {
        format!("ldm-helper-{target_triple}")
    };
    let dest = binaries_dir.join(dest_name);

    let bin_name = if is_windows { "ldm-helper.exe" } else { "ldm-helper" };
    let built_helper = manifest_dir.join("target").join(&profile).join(bin_name);

    if built_helper.exists() {
        if let Err(e) = std::fs::copy(&built_helper, &dest) {
            println!("cargo:warning=failed to copy ldm-helper sidecar to {dest:?}: {e}");
            return;
        }
        println!("cargo:rerun-if-changed={}", built_helper.display());
    } else {
        println!(
            "cargo:warning=ldm-helper binary not found at {built_helper:?} — dropping a placeholder at {dest:?}. Run `cargo build --workspace` (or `-p ldm-helper`) first, then rebuild, to ship the real helper."
        );
        write_placeholder(&dest, is_windows);
    }
}

fn write_placeholder(dest: &std::path::Path, is_windows: bool) {
    let contents = if is_windows {
        String::new()
    } else {
        "#!/bin/sh\necho \"ldm-helper placeholder: run 'cargo build --workspace' (or '-p ldm-helper') and rebuild before packaging\" >&2\nexit 1\n".to_string()
    };
    if let Err(e) = std::fs::write(dest, contents) {
        println!("cargo:warning=failed to write ldm-helper placeholder at {dest:?}: {e}");
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(dest) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(dest, perms);
        }
    }
}
