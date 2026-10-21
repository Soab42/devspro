//! The "New project" wizard: one command that runs the whole set-up in order
//! and reports each step separately.
//!
//! Every step is its own small function returning `Result<String, String>` so a
//! failure is attributable to exactly one thing ("database" failed, the site
//! and cert are fine). The runner stops at the first failure but still returns
//! everything that completed before it, so the UI can show a green list with
//! one red row rather than a single opaque error — and the user knows what
//! does *not* need redoing.
//!
//! Nothing here re-implements another module's logic: scaffolding is
//! `super::create_laravel_skeleton`, the site (vhost + cert + /etc/hosts sync)
//! is `nginx_ssl::sites::nginx_ssl_add_site`, hosts verification uses the
//! Hosts file module's parser, and the database goes through
//! `databases::databases_create_database`.

use super::env_patch;
use crate::modules::databases;
use crate::modules::hosts_file;
use crate::modules::nginx_ssl::{laravel, php_fpm, sites};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    /// `composer create-project laravel/laravel`.
    Laravel,
    /// `<parent_dir>/<name>` already exists; skip scaffolding.
    Existing,
    /// Just an empty folder.
    Blank,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WizardPlan {
    pub name: String,
    pub parent_dir: String,
    pub template: Template,
    pub domain: String,
    #[serde(default)]
    pub php_version: Option<String>,
    #[serde(default)]
    pub create_database: bool,
    #[serde(default)]
    pub db_connection: Option<String>,
    #[serde(default)]
    pub db_name: Option<String>,
    #[serde(default)]
    pub write_env: bool,
    #[serde(default)]
    pub run_migrations: bool,
    #[serde(default)]
    pub open_in_editor: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StepReport {
    pub step: String,
    pub ok: bool,
    pub message: String,
}

/// Step identifiers, in execution order. The frontend mirrors this list to
/// show pending rows before the command returns.
pub const STEP_SCAFFOLD: &str = "scaffold";
pub const STEP_SITE: &str = "site";
pub const STEP_HOSTS: &str = "hosts";
pub const STEP_DATABASE: &str = "database";
pub const STEP_ENV: &str = "env";
pub const STEP_ARTISAN: &str = "artisan";
pub const STEP_EDITOR: &str = "editor";

/// Mutable state threaded through the steps: what earlier steps produced
/// that later ones need (the project path, the `.env` values to write).
struct Ctx<'a> {
    app: AppHandle,
    db: State<'a, crate::db::Db>,
    plan: WizardPlan,
    project_dir: PathBuf,
    env_updates: Vec<(String, String)>,
}

/// Which steps a plan will run, in order. Pure, so the frontend's expectation
/// and the backend's execution can be checked against each other in tests.
pub fn planned_steps(plan: &WizardPlan) -> Vec<&'static str> {
    let mut steps = vec![STEP_SCAFFOLD, STEP_SITE, STEP_HOSTS];
    if plan.create_database {
        steps.push(STEP_DATABASE);
    }
    if plan.write_env {
        steps.push(STEP_ENV);
    }
    if plan.run_migrations {
        steps.push(STEP_ARTISAN);
    }
    if plan.open_in_editor.as_deref().is_some_and(|e| !e.trim().is_empty()) {
        steps.push(STEP_EDITOR);
    }
    steps
}

fn validate(plan: &WizardPlan) -> Result<(), String> {
    if !super::valid_project_name(plan.name.trim()) {
        return Err(
            "project name must be 1-100 characters of letters, numbers, - or _, and can't start with - or _"
                .to_string(),
        );
    }
    if plan.domain.trim().is_empty() {
        return Err("a domain is required".to_string());
    }
    if plan.create_database {
        if plan.db_connection.as_deref().unwrap_or("").trim().is_empty() {
            return Err("choose a database server to create the database on".to_string());
        }
        if plan.db_name.as_deref().unwrap_or("").trim().is_empty() {
            return Err("a database name is required".to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

fn step_scaffold(ctx: &mut Ctx) -> Result<String, String> {
    let name = ctx.plan.name.trim();
    match ctx.plan.template {
        Template::Laravel => {
            ctx.project_dir = super::create_laravel_skeleton(&ctx.plan.parent_dir, name)?;
            Ok(format!("created Laravel project at {}", ctx.project_dir.display()))
        }
        Template::Blank => {
            if ctx.project_dir.exists() {
                return Err(format!("{} already exists", ctx.project_dir.display()));
            }
            std::fs::create_dir_all(&ctx.project_dir)
                .map_err(|e| format!("could not create {}: {e}", ctx.project_dir.display()))?;
            Ok(format!("created empty folder {}", ctx.project_dir.display()))
        }
        Template::Existing => {
            if !ctx.project_dir.is_dir() {
                return Err(format!("{} is not a directory", ctx.project_dir.display()));
            }
            Ok(format!("using existing folder {}", ctx.project_dir.display()))
        }
    }
}

/// Registers the nginx site. `nginx_ssl_add_site` only knows Laravel and
/// port-proxy backends, so a folder without `artisan` (a blank project, or an
/// existing non-Laravel one) has nothing to point nginx at yet — that is
/// reported as a skip rather than a failure, since every later step is still
/// meaningful.
fn step_site(ctx: &mut Ctx) -> Result<String, String> {
    if !laravel::is_laravel_project(&ctx.project_dir) {
        return Ok("skipped — not a Laravel project yet; add a proxy-to-port site later from \"Add site\"".to_string());
    }
    let site = sites::nginx_ssl_add_site(
        ctx.app.clone(),
        ctx.db.clone(),
        ctx.plan.name.trim().to_string(),
        ctx.plan.domain.trim().to_string(),
        "laravel".to_string(),
        None,
        Some(ctx.project_dir.to_string_lossy().to_string()),
        ctx.plan.php_version.clone().filter(|v| !v.trim().is_empty()),
    )?;
    Ok(format!("https://{} is live with a trusted certificate", site.hostname))
}

/// `nginx_ssl_add_site` already rewrites the devspro block in /etc/hosts, so
/// after a successful site step this only verifies. When the site step was
/// skipped the domain is added as a DevsPro-tagged line through the Hosts file
/// module, so the folder is at least reachable by name once a site exists.
fn step_hosts(ctx: &mut Ctx) -> Result<String, String> {
    let domain = ctx.plan.domain.trim().to_lowercase();
    let path = hosts_file::hosts_path();
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    if hosts_file::parse(&contents).loopback_hostnames().contains(&domain) {
        return Ok(format!("{domain} resolves to 127.0.0.1"));
    }
    hosts_file::hosts_file_add(ctx.app.clone(), "127.0.0.1".to_string(), domain.clone())?;
    Ok(format!("added {domain} to {}", path.display()))
}

fn step_database(ctx: &mut Ctx) -> Result<String, String> {
    let connection = ctx.plan.db_connection.clone().unwrap_or_default();
    let db_name = ctx.plan.db_name.clone().unwrap_or_default();
    let message = databases::databases_create_database(ctx.app.clone(), connection.clone(), db_name.clone())?;
    if ctx.plan.write_env {
        ctx.env_updates
            .extend(databases::laravel_env_for(&ctx.app, connection.trim(), db_name.trim())?);
    }
    Ok(message)
}

/// Starts from `.env.example` when there is no `.env` yet — that is what
/// `composer create-project` does too, and an existing project checked out
/// fresh from git is in exactly that state.
fn step_env(ctx: &mut Ctx) -> Result<String, String> {
    let env_path = ctx.project_dir.join(".env");
    let contents = if env_path.is_file() {
        std::fs::read_to_string(&env_path).map_err(|e| format!("could not read {}: {e}", env_path.display()))?
    } else {
        let example = ctx.project_dir.join(".env.example");
        if !example.is_file() {
            return Err(format!(
                "no .env or .env.example in {} — nothing to write to",
                ctx.project_dir.display()
            ));
        }
        std::fs::read_to_string(&example).map_err(|e| format!("could not read {}: {e}", example.display()))?
    };

    let mut updates = vec![("APP_URL".to_string(), format!("https://{}", ctx.plan.domain.trim().to_lowercase()))];
    updates.append(&mut ctx.env_updates);
    let patched = env_patch::patch_env(&contents, &updates);
    std::fs::write(&env_path, patched).map_err(|e| format!("could not write {}: {e}", env_path.display()))?;

    let keys: Vec<&str> = updates.iter().map(|(k, _)| k.as_str()).collect();
    Ok(format!("wrote {} ({})", env_path.display(), keys.join(", ")))
}

/// The PHP the site is served with, so artisan runs under the same version:
/// `php8.3` on Debian/Ubuntu, the `php.exe` beside the site's `php-cgi.exe` on
/// Windows. Resolved by `php_fpm`, which already knows where each version lives.
fn php_binary(php_version: Option<&str>) -> String {
    php_fpm::cli_binary(php_version)
}

fn run_artisan(project_dir: &Path, php: &str, args: &[&str]) -> Result<(), String> {
    let output = crate::modules::tools::command(php)
        .arg("artisan")
        .args(args)
        .arg("--no-interaction")
        .current_dir(project_dir)
        .output()
        .map_err(|e| format!("failed to run {php} artisan {}: {e}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(format!("artisan {} failed: {}", args.join(" "), if detail.is_empty() { "unknown error".into() } else { detail }))
}

fn step_artisan(ctx: &mut Ctx) -> Result<String, String> {
    if !ctx.project_dir.join("artisan").is_file() {
        return Err("no artisan file in the project — migrations need a Laravel project".to_string());
    }
    let php = php_binary(ctx.plan.php_version.as_deref());
    let env_path = ctx.project_dir.join(".env");
    let env = std::fs::read_to_string(&env_path).unwrap_or_default();
    // Composer's create-project already generated a key; regenerating would
    // invalidate anything encrypted with it. `--force` is still needed for
    // the "fresh checkout" case where APP_KEY is genuinely empty.
    let mut ran = Vec::new();
    if env_patch::value_of(&env, "APP_KEY").unwrap_or_default().is_empty() {
        run_artisan(&ctx.project_dir, &php, &["key:generate", "--force"])?;
        ran.push("key:generate");
    }
    run_artisan(&ctx.project_dir, &php, &["migrate", "--force"])?;
    ran.push("migrate");
    Ok(format!("ran {} with {php}", ran.join(" and ")))
}

fn step_editor(ctx: &mut Ctx) -> Result<String, String> {
    let editor = ctx.plan.open_in_editor.clone().unwrap_or_default().trim().to_string();
    // Resolved through `tools` because the usual answers are shims on Windows —
    // VS Code's `code` is `code.cmd` — which a bare name can't start there. A full
    // path, or a name not on PATH, is passed through as typed.
    let program = crate::modules::tools::program(&editor).unwrap_or_else(|| editor.clone());
    crate::modules::tools::command(&program)
        .arg(&ctx.project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not launch {editor}: {e}"))?;
    Ok(format!("opened in {editor}"))
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

type Step = fn(&mut Ctx) -> Result<String, String>;

fn step_fn(id: &str) -> Step {
    match id {
        STEP_SCAFFOLD => step_scaffold,
        STEP_SITE => step_site,
        STEP_HOSTS => step_hosts,
        STEP_DATABASE => step_database,
        STEP_ENV => step_env,
        STEP_ARTISAN => step_artisan,
        STEP_EDITOR => step_editor,
        other => unreachable!("unknown wizard step {other}"),
    }
}

/// Runs `steps` in order, stopping at the first failure. Generic over the step
/// runner so the stop-at-first-failure contract is testable without a Tauri
/// app handle.
fn run_steps<'s, S>(steps: &[&'s str], mut state: S, run: impl Fn(&'s str, &mut S) -> Result<String, String>) -> Vec<StepReport> {
    let mut report = Vec::with_capacity(steps.len());
    for &step in steps {
        match run(step, &mut state) {
            Ok(message) => report.push(StepReport { step: step.to_string(), ok: true, message }),
            Err(message) => {
                report.push(StepReport { step: step.to_string(), ok: false, message });
                break;
            }
        }
    }
    report
}

#[tauri::command(async)]
pub fn scaffold_wizard_run(app: AppHandle, db: State<crate::db::Db>, plan: WizardPlan) -> Result<Vec<StepReport>, String> {
    validate(&plan)?;
    let project_dir = Path::new(&plan.parent_dir).join(plan.name.trim());
    let steps = planned_steps(&plan);
    let ctx = Ctx { app, db, plan, project_dir, env_updates: Vec::new() };
    Ok(run_steps(&steps, ctx, |step, ctx| step_fn(step)(ctx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> WizardPlan {
        WizardPlan {
            name: "shop".into(),
            parent_dir: "/tmp".into(),
            template: Template::Laravel,
            domain: "shop.test".into(),
            php_version: None,
            create_database: false,
            db_connection: None,
            db_name: None,
            write_env: false,
            run_migrations: false,
            open_in_editor: None,
        }
    }

    #[test]
    fn planned_steps_follow_the_switches_in_order() {
        assert_eq!(planned_steps(&plan()), vec![STEP_SCAFFOLD, STEP_SITE, STEP_HOSTS]);
        let full = WizardPlan {
            create_database: true,
            write_env: true,
            run_migrations: true,
            open_in_editor: Some("code".into()),
            ..plan()
        };
        assert_eq!(
            planned_steps(&full),
            vec![STEP_SCAFFOLD, STEP_SITE, STEP_HOSTS, STEP_DATABASE, STEP_ENV, STEP_ARTISAN, STEP_EDITOR]
        );
        let blank_editor = WizardPlan { open_in_editor: Some("  ".into()), ..plan() };
        assert!(!planned_steps(&blank_editor).contains(&STEP_EDITOR));
    }

    #[test]
    fn validate_requires_db_details_only_when_creating_one() {
        assert!(validate(&plan()).is_ok());
        let missing = WizardPlan { create_database: true, ..plan() };
        assert!(validate(&missing).unwrap_err().contains("database server"));
        let no_name = WizardPlan { create_database: true, db_connection: Some("mysql8".into()), ..plan() };
        assert!(validate(&no_name).unwrap_err().contains("database name"));
        let bad_name = WizardPlan { name: "../x".into(), ..plan() };
        assert!(validate(&bad_name).is_err());
        let no_domain = WizardPlan { domain: " ".into(), ..plan() };
        assert!(validate(&no_domain).is_err());
    }

    #[test]
    fn run_steps_stops_at_the_first_failure_but_keeps_earlier_results() {
        let steps = ["a", "b", "c"];
        let report = run_steps(&steps, 0u32, |step, count| {
            *count += 1;
            if step == "b" { Err("boom".into()) } else { Ok(format!("{step} done")) }
        });
        assert_eq!(
            report,
            vec![
                StepReport { step: "a".into(), ok: true, message: "a done".into() },
                StepReport { step: "b".into(), ok: false, message: "boom".into() },
            ]
        );
    }

    #[test]
    fn plan_deserializes_with_optional_fields_absent() {
        let json = r#"{"name":"shop","parent_dir":"/tmp","template":"existing","domain":"shop.test"}"#;
        let plan: WizardPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.template, Template::Existing);
        assert!(!plan.create_database && !plan.write_env && !plan.run_migrations);
        assert_eq!(plan.php_version, None);
    }

    #[test]
    fn php_binary_falls_back_to_plain_php() {
        assert_eq!(php_binary(None), "php");
        assert_eq!(php_binary(Some("0.0-does-not-exist")), "php");
    }
}
