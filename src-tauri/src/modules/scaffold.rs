//! One-click "new project" scaffolding: creates a fresh Laravel skeleton on
//! disk via Composer, then delegates the actual site-wiring (hostname
//! validation, cert issuance, nginx conf, /etc/hosts, PHP-FPM check) to
//! `nginx_ssl::sites::nginx_ssl_add_site` rather than re-implementing any of
//! it here. This module only owns what's specific to "create the project
//! files in the first place."

use super::nginx_ssl::{laravel, sites};
use sites::Site;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, State};

fn valid_project_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 {
        return false;
    }
    let ok_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok_chars && !name.starts_with('-') && !name.starts_with('_')
}

#[tauri::command]
pub fn scaffold_create_laravel_project(
    app: AppHandle,
    db: State<crate::db::Db>,
    parent_dir: String,
    project_name: String,
    hostname: String,
) -> Result<Site, String> {
    let project_name = project_name.trim().to_string();
    if !valid_project_name(&project_name) {
        return Err(
            "project name must be 1-100 characters of letters, numbers, - or _, and can't start with - or _"
                .to_string(),
        );
    }

    let parent = Path::new(&parent_dir);
    if !parent.is_dir() {
        return Err(format!("{parent_dir} is not a directory"));
    }

    let target: PathBuf = parent.join(&project_name);
    if target.exists() {
        return Err(format!(
            "{} already exists — choose a different project name or parent folder",
            target.display()
        ));
    }

    // Composer isn't in this app's Apps catalog (see modules/apps/mod.rs) — only
    // PHP itself is tracked there — so this can't point the user at an "install
    // from here" button yet, just a direct instruction.
    // Resolved rather than merely checked for, because Composer ships as `composer.bat` on
    // Windows and a bare `composer` is not something `Command` can start there — the check
    // would pass and the run would fail. On Unix this is the same bare name as before.
    let composer = crate::modules::tools::program("composer").ok_or(
        "Composer was not found on this machine's PATH — install it first (see https://getcomposer.org/download/), then try again",
    )?;

    // `composer create-project` over `laravel new`: it only needs Composer
    // itself, which is the one tool actually checked above. `laravel new`
    // requires the separate `laravel/installer` package to already be
    // globally required, which this app has no way to detect or install, so
    // it would fail unpredictably on a machine that only has Composer.
    let output = Command::new(&composer)
        .args([
            "create-project",
            "laravel/laravel",
            &target.to_string_lossy(),
            "--no-interaction",
        ])
        .output()
        .map_err(|e| format!("failed to run composer: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!(
            "composer create-project failed: {}",
            if detail.is_empty() { "unknown error".to_string() } else { detail }
        ));
    }

    if !laravel::is_laravel_project(&target) {
        return Err(format!(
            "composer finished but {} doesn't look like a Laravel project (missing artisan or public/index.php) — check it manually",
            target.display()
        ));
    }

    // Delegate the entire registration step — the project directory already
    // exists on disk at this point, so a failure here (e.g. hostname taken)
    // is surfaced with the exact path rather than being cleaned up: retrying
    // through the existing "Add site" UI pointed at that folder is simpler
    // and safer than this command guessing whether it's safe to `rm -rf` a
    // directory it just created.
    sites::nginx_ssl_add_site(
        app,
        db,
        project_name,
        hostname,
        "laravel".to_string(),
        None,
        Some(target.to_string_lossy().to_string()),
        None,
    )
    .map_err(|e| {
        format!(
            "project was created at {} but registering it as a site failed: {e} — you can retry from the Sites panel's \"Add site\" form pointed at that folder",
            target.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_names() {
        assert!(valid_project_name("myapp"));
        assert!(valid_project_name("my-app"));
        assert!(valid_project_name("my_app_2"));
        assert!(valid_project_name("A1"));
    }

    #[test]
    fn rejects_empty_and_too_long() {
        assert!(!valid_project_name(""));
        assert!(!valid_project_name(&"a".repeat(101)));
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        assert!(!valid_project_name(".."));
        assert!(!valid_project_name("."));
        assert!(!valid_project_name("../etc"));
        assert!(!valid_project_name("foo/bar"));
        assert!(!valid_project_name("foo\\bar"));
        assert!(!valid_project_name("/etc/passwd"));
    }

    #[test]
    fn rejects_leading_dash_or_underscore() {
        assert!(!valid_project_name("-app"));
        assert!(!valid_project_name("_app"));
    }

    #[test]
    fn rejects_disallowed_characters() {
        assert!(!valid_project_name("my app"));
        assert!(!valid_project_name("my.app"));
        assert!(!valid_project_name("app!"));
    }
}
