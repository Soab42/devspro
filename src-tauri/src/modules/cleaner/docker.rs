//! Docker tab: stopped containers, dangling and unused images, unused
//! networks, build cache and dangling volumes, one finding per item. Talks to
//! the `docker` CLI like the Docker module does and never elevates. Volumes
//! that belong to DevsPro's own Databases and Mail Catcher stacks need their
//! name typed and are never suggested.

mod ids;
mod parse;

use super::{history, measure, CleanOutcome, CleanerState, Finding, Risk, Scan};
use crate::db::Db;
use crate::modules::docker::detect;
use chrono::{DateTime, Duration, Utc};
use ids::{image_argv, is_confirmed, is_protected, plan, DockerId, COMPOSE_PROJECT_LABEL};
use parse::{cache_totals, parse_containers, parse_df, parse_networks, parse_volumes, reclaimed, short_id, Df};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, State};

const TAB: &str = "docker";
const CACHE_DAYS: u32 = 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerScan {
    /// False when Docker is missing or its daemon is not running.
    pub available: bool,
    /// Why it is unavailable.
    pub message: Option<String>,
    pub findings: Vec<Finding>,
}

/// Runs `docker <args>` and returns stdout, or docker's own error text.
type Runner<'a> = &'a dyn Fn(&[&str]) -> Result<String, String>;

fn run_docker(args: &[&str]) -> Result<String, String> {
    let out = crate::modules::tools::command("docker").args(args).output().map_err(|e| format!("failed to run docker: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("docker {} failed", args.first().unwrap_or(&"")) } else { err })
    }
}

fn finding(id: &DockerId, group: &str, label: String, reason: String, method: &str, bytes: u64, risk: Risk) -> Finding {
    Finding {
        id: id.id(),
        tab: TAB.to_string(),
        group: group.to_string(),
        label,
        reason,
        method: method.to_string(),
        paths: Vec::new(),
        bytes,
        risk,
        suggested: false,
        blocked_by: None,
        needs_confirm_text: None,
    }
}

fn scan_with(run: Runner, scan: &Scan, now: DateTime<Utc>) -> Result<Vec<Finding>, String> {
    let df: Df = parse_df(&run(&["system", "df", "-v", "--format", "json"])?)?;
    let mut out: Vec<Finding> = Vec::new();

    let sizes = df.container_sizes();
    let stopped = run(&[
        "ps", "-a", "--no-trunc", "--filter", "status=exited", "--filter", "status=created", "--format", "{{json .}}",
    ])?;
    for c in parse_containers(&stopped)? {
        let id = DockerId::Container(c.id.clone());
        let bytes = sizes.get(short_id(&c.id)).copied().unwrap_or(0);
        out.push(finding(
            &id,
            "Stopped containers",
            format!("{} ({})", c.names, c.image),
            c.status.clone(),
            "docker rm",
            bytes,
            Risk::Review,
        ));
    }
    if scan.is_cancelled() {
        return Ok(out);
    }

    for img in df.images() {
        let id = DockerId::Image(img.id.clone());
        if img.dangling() && img.containers == 0 {
            let mut f = finding(&id, "Dangling images", format!("Untagged image {}", img.id), "Left behind by a rebuild; nothing uses it.".into(), "docker image rm", img.bytes, Risk::Safe);
            f.suggested = true;
            out.push(f);
        } else if img.containers == 0 {
            out.push(finding(&id, "Unused images", img.tags.join(", "), "No container, running or stopped, uses it; pulling it again costs a download.".into(), "docker image rm", img.bytes, Risk::Review));
        }
    }

    let networks = run(&["network", "ls", "--filter", "dangling=true", "--format", "{{json .}}"])?;
    for n in parse_networks(&networks)? {
        let id = DockerId::Network(n.id.clone());
        let mut f = finding(&id, "Unused networks", n.name, "No container is attached.".into(), "docker network rm", 0, Risk::Safe);
        f.suggested = true;
        out.push(f);
    }

    let (all, old) = cache_totals(&df.cache(), now - Duration::days(i64::from(CACHE_DAYS)));
    if all > 0 {
        out.push(finding(&DockerId::BuildCacheAll, "Build cache", "All build cache".into(), "Rebuilt on the next image build, which then takes longer.".into(), "docker builder prune", all, Risk::Safe));
    }
    if old > 0 {
        let mut f = finding(&DockerId::BuildCacheOlder(CACHE_DAYS), "Build cache", format!("Build cache older than {CACHE_DAYS} days"), "Not used for a week; rebuilt if ever needed.".into(), "docker builder prune", old, Risk::Safe);
        f.suggested = true;
        out.push(f);
    }
    if scan.is_cancelled() {
        return Ok(out);
    }

    let volume_sizes = df.volume_sizes();
    let volumes = run(&["volume", "ls", "--filter", "dangling=true", "--format", "{{json .}}"])?;
    for v in parse_volumes(&volumes)? {
        let project = v.project();
        let protected = is_protected(&v.name, project.as_deref());
        let owner = match &project {
            Some(p) => format!("Compose project: {p}"),
            None => "Not part of a compose project".to_string(),
        };
        let reason = if protected {
            format!("{owner}. Belongs to a DevsPro stack and holds its data; type the name to remove it.")
        } else {
            format!("{owner}. No container uses it; removing it deletes its data for good.")
        };
        let bytes = volume_sizes.get(&v.name).copied().unwrap_or(0);
        let mut f = finding(&DockerId::Volume(v.name.clone()), "Volumes", v.name.clone(), reason, "docker volume rm", bytes, Risk::Danger);
        if protected {
            f.needs_confirm_text = Some(v.name);
        }
        out.push(f);
    }
    Ok(out)
}

/// The docker data root, when it is a folder on this machine. Docker Desktop
/// on Windows (and macOS) reports `/var/lib/docker` inside its Linux VM, which
/// is not a path here; the freed space then comes from docker's own sizes.
fn data_root(run: Runner) -> Option<String> {
    let root = run(&["info", "--format", "{{.DockerRootDir}}"]).ok()?;
    let root = root.trim();
    (!root.is_empty() && Path::new(root).is_absolute()).then(|| root.to_string())
}

fn project_of(run: Runner, name: &str) -> Result<Option<String>, String> {
    let format = format!("{{{{index .Labels \"{COMPOSE_PROJECT_LABEL}\"}}}}");
    let out = run(&["volume", "inspect", "--format", &format, name])?;
    let out = out.trim();
    Ok((!out.is_empty() && out != "<no value>").then(|| out.to_string()))
}

/// The `repo:tag` names an image carries; empty when untagged or unreadable.
fn image_tags(run: Runner, id: &str) -> Vec<String> {
    run(&["image", "inspect", "--format", "{{json .RepoTags}}", id])
        .ok()
        .and_then(|t| serde_json::from_str::<Option<Vec<String>>>(t.trim()).ok().flatten())
        .unwrap_or_default()
}

/// Size docker reported for one item, for when free space cannot be read.
fn reported_bytes(id: &DockerId, df: &Option<Df>) -> u64 {
    let Some(df) = df else { return 0 };
    match id {
        DockerId::Container(i) => df.container_sizes().get(short_id(i)).copied().unwrap_or(0),
        DockerId::Image(i) => df.images().iter().find(|img| img.id == short_id(i)).map_or(0, |img| img.bytes),
        DockerId::Volume(n) => df.volume_sizes().get(n).copied().unwrap_or(0),
        DockerId::Network(_) | DockerId::BuildCacheAll | DockerId::BuildCacheOlder(_) => 0,
    }
}

fn clean_with(
    ids: &[String],
    confirmed: &[String],
    run: Runner,
    free: &dyn Fn() -> Option<u64>,
) -> Result<Vec<CleanOutcome>, String> {
    let steps = plan(ids)?;
    let df = run(&["system", "df", "-v", "--format", "json"]).ok().and_then(|t| parse_df(&t).ok());
    let mut outcomes = Vec::new();

    for step in steps {
        let id = step.id();
        let fail = |message: String| CleanOutcome { id: id.clone(), ok: false, message, bytes_freed: 0 };

        if let DockerId::Volume(name) = &step {
            match project_of(run, name) {
                Err(e) => {
                    outcomes.push(fail(e));
                    continue;
                }
                Ok(project) if is_protected(name, project.as_deref()) && !is_confirmed(name, confirmed) => {
                    outcomes.push(fail("Belongs to a DevsPro stack; its name was not confirmed.".to_string()));
                    continue;
                }
                Ok(_) => {}
            }
        }

        let argv = match &step {
            DockerId::Image(i) => image_argv(i, &image_tags(run, i)),
            _ => step.argv(),
        };
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let before = free();
        outcomes.push(match run(&argv) {
            Err(e) => fail(e),
            Ok(out) => {
                let reported = match step {
                    DockerId::BuildCacheAll | DockerId::BuildCacheOlder(_) => reclaimed(&out),
                    _ => reported_bytes(&step, &df),
                };
                let bytes_freed = match (before, free()) {
                    (Some(b), Some(a)) => a.saturating_sub(b),
                    _ => reported,
                };
                CleanOutcome { id: id.clone(), ok: true, message: "Removed".to_string(), bytes_freed }
            }
        });
    }
    Ok(outcomes)
}

#[tauri::command(async)]
pub fn cleaner_scan_docker(
    app: AppHandle,
    state: State<CleanerState>,
    _db: State<Db>,
    scan_id: String,
) -> Result<DockerScan, String> {
    let scan = Scan::start(&state, &app, &scan_id, TAB);
    let d = detect();
    if !d.found || !d.daemon_reachable {
        let message = if !d.found {
            "Docker isn't installed.".to_string()
        } else {
            d.error.filter(|e| !e.is_empty()).unwrap_or_else(|| "Could not reach the Docker daemon.".to_string())
        };
        return Ok(DockerScan { available: false, message: Some(message), findings: Vec::new() });
    }
    let findings = scan_with(&run_docker, &scan, Utc::now())?;
    Ok(DockerScan { available: true, message: None, findings })
}

/// `confirmed` lists the ids whose typed confirmation text matched.
#[tauri::command(async)]
pub fn cleaner_clean_docker(
    _app: AppHandle,
    db: State<Db>,
    ids: Vec<String>,
    confirmed: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    let root = data_root(&run_docker);
    let free = || root.as_deref().and_then(|r| measure::free_bytes(Path::new(r)).ok());
    let outcomes = clean_with(&ids, &confirmed, &run_docker, &free)?;
    if let Ok(conn) = db.0.lock() {
        for o in &outcomes {
            // A history write failing must not hide that the removal happened.
            let _ = history::record(&conn, TAB, &o.id, o.bytes_freed, o.ok, &o.message);
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const C1: &str = "bd830e46b3c6aaaa";
    const IMG: &str = "3c988764b52a";

    fn scan_inputs(args: &[&str]) -> Result<String, String> {
        Ok(match args[0] {
            "system" => format!(
                r#"{{"Images":[
                  {{"ID":"sha256:{IMG}ffff","Repository":"app","Tag":"old","Size":"400MB","UniqueSize":"100MB","Containers":"0"}},
                  {{"ID":"sha256:111111111111ffff","Repository":"<none>","Tag":"<none>","Size":"9MB","UniqueSize":"9MB","Containers":"0"}},
                  {{"ID":"sha256:222222222222ffff","Repository":"web","Tag":"1","Size":"5MB","UniqueSize":"5MB","Containers":"1"}}],
                 "Containers":[{{"ID":"{C1}","Size":"2kB"}}],
                 "Volumes":[{{"Name":"devspro_pgdata","Size":"50MB"}},{{"Name":"scratch","Size":"1MB"}}],
                 "BuildCache":[
                  {{"InUse":"false","LastUsedAt":"2026-01-01 00:00:00 +0000 UTC","Size":"30MB"}},
                  {{"InUse":"false","LastUsedAt":"2026-10-06 00:00:00 +0000 UTC","Size":"5MB"}}]}}"#
            ),
            "ps" => format!(r#"{{"ID":"{C1}","Names":"web-1","Image":"web","Status":"Exited (0) 2 days ago"}}"#),
            "network" => r#"{"ID":"a9ae4e7f62e2","Name":"bc-net"}
{"ID":"79d060342d7c","Name":"bridge"}"#
                .to_string(),
            "volume" => r#"{"Name":"devspro_pgdata","Labels":"com.docker.compose.project=devspro"}
{"Name":"scratch","Labels":"com.docker.compose.project=other"}"#
                .to_string(),
            _ => return Err(format!("unexpected {args:?}")),
        })
    }

    fn now() -> DateTime<Utc> {
        "2026-10-07T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn scan_makes_one_finding_per_item_with_the_right_risk() {
        let f = scan_with(&scan_inputs, &Scan::detached(), now()).unwrap();
        let by = |id: &str| f.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("missing {id}"));

        let c = by(&format!("container:{C1}"));
        assert_eq!((c.risk, c.suggested, c.group.as_str()), (Risk::Review, false, "Stopped containers"));
        assert!(c.label.contains("web-1") && c.label.contains("web") && c.reason.contains("Exited"));
        assert_eq!(c.bytes, 2_000);

        let dangling = by("image:111111111111");
        assert_eq!((dangling.risk, dangling.suggested), (Risk::Safe, true));
        let unused = by(&format!("image:{IMG}"));
        assert_eq!((unused.risk, unused.suggested, unused.bytes), (Risk::Review, false, 100_000_000));
        assert!(f.iter().all(|x| x.id != "image:222222222222"), "an image a container uses is not listed");

        let net = by("network:a9ae4e7f62e2");
        assert_eq!(net.risk, Risk::Safe);
        assert!(f.iter().all(|x| x.id != "network:79d060342d7c"));

        assert_eq!(by("buildcache:all").bytes, 35_000_000);
        assert_eq!(by("buildcache:older:7").bytes, 30_000_000);
        assert!(by("buildcache:older:7").suggested && !by("buildcache:all").suggested);
    }

    #[test]
    fn scan_protects_devspro_volumes_and_never_suggests_volumes() {
        let f = scan_with(&scan_inputs, &Scan::detached(), now()).unwrap();
        let own = f.iter().find(|x| x.id == "volume:devspro_pgdata").unwrap();
        assert_eq!(own.needs_confirm_text.as_deref(), Some("devspro_pgdata"));
        assert_eq!((own.risk, own.suggested, own.bytes), (Risk::Danger, false, 50_000_000));
        assert!(own.reason.contains("devspro"));
        let other = f.iter().find(|x| x.id == "volume:scratch").unwrap();
        assert_eq!((other.risk, other.suggested, other.needs_confirm_text.clone()), (Risk::Danger, false, None));
        assert!(other.reason.contains("other"));
        assert!(f.iter().filter(|x| x.group == "Volumes").all(|x| !x.suggested));
    }

    /// A runner that records every call and answers the read-only ones.
    fn recording<'a>(calls: &'a RefCell<Vec<Vec<String>>>, project: &'a str) -> impl Fn(&[&str]) -> Result<String, String> + 'a {
        move |args: &[&str]| {
            calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            match args[0] {
                "system" => Ok(scan_inputs(args)?),
                "volume" if args[1] == "inspect" => Ok(format!("{project}\n")),
                "builder" => Ok("Total reclaimed space: 12MB\n".to_string()),
                _ => Ok(String::new()),
            }
        }
    }

    fn mutating(calls: &RefCell<Vec<Vec<String>>>) -> Vec<Vec<String>> {
        calls.borrow().iter().filter(|c| !matches!(c[0].as_str(), "system") && c.get(1).map(String::as_str) != Some("inspect")).cloned().collect()
    }

    #[test]
    fn clean_runs_in_removal_order_with_fixed_argv() {
        let calls = RefCell::new(Vec::new());
        let ids: Vec<String> = ["buildcache:older:7", "volume:scratch", "network:a9ae4e7f62e2", "image:3c988764b52a", &format!("container:{C1}")]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let run = recording(&calls, "other");
        let out = clean_with(&ids, &[], &run, &|| None).unwrap();
        assert!(out.iter().all(|o| o.ok));
        let ran = mutating(&calls);
        let expect: Vec<Vec<&str>> = vec![
            vec!["rm", C1],
            vec!["image", "rm", "3c988764b52a"],
            vec!["network", "rm", "a9ae4e7f62e2"],
            vec!["volume", "rm", "scratch"],
            vec!["builder", "prune", "--all", "--force", "--filter", "until=168h"],
        ];
        assert_eq!(ran, expect);
        assert_eq!(out[0].bytes_freed, 2_000, "reported size when free space is unreadable");
        assert_eq!(out[1].bytes_freed, 100_000_000);
        assert_eq!(out[3].bytes_freed, 1_000_000);
        assert_eq!(out[4].bytes_freed, 12_000_000);
    }

    #[test]
    fn a_tagged_image_is_removed_by_each_tag() {
        let calls = RefCell::new(Vec::new());
        let inner = recording(&calls, "");
        let run = |args: &[&str]| -> Result<String, String> {
            if args[..2] == ["image", "inspect"] {
                calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
                return Ok(r#"["app:old","app:v2"]"#.to_string());
            }
            inner(args)
        };
        let out = clean_with(&["image:3c988764b52a".to_string()], &[], &run, &|| None).unwrap();
        assert!(out[0].ok);
        assert_eq!(mutating(&calls), vec![vec!["image", "rm", "app:old", "app:v2"]]);
    }

    #[test]
    fn clean_prefers_free_space_when_readable() {
        let calls = RefCell::new(Vec::new());
        let readings = RefCell::new(vec![5_000u64, 1_000].into_iter().collect::<Vec<_>>());
        let free = || readings.borrow_mut().pop();
        let run = recording(&calls, "");
        let out = clean_with(&[format!("container:{C1}")], &[], &run, &free).unwrap();
        assert_eq!(out[0].bytes_freed, 4_000);
    }

    #[test]
    fn protected_volume_needs_its_name_in_confirmed() {
        for project in ["devspro", "devspro_mailcatcher"] {
            let calls = RefCell::new(Vec::new());
            let run = recording(&calls, project);
            let ids = vec!["volume:devspro_pgdata".to_string()];

            let refused = clean_with(&ids, &[], &run, &|| None).unwrap();
            assert!(!refused[0].ok);
            assert!(mutating(&calls).is_empty(), "nothing may run unconfirmed");

            let wrong = clean_with(&ids, &["volume:devspro_pg".to_string()], &run, &|| None).unwrap();
            assert!(!wrong[0].ok && mutating(&calls).is_empty());

            let done = clean_with(&ids, &["volume:devspro_pgdata".to_string()], &run, &|| None).unwrap();
            assert!(done[0].ok);
            assert_eq!(mutating(&calls), vec![vec!["volume", "rm", "devspro_pgdata"]]);
        }
    }

    #[test]
    fn protection_follows_the_volumes_label_not_just_its_name() {
        let calls = RefCell::new(Vec::new());
        let run = recording(&calls, "devspro");
        let out = clean_with(&["volume:innocent_name".to_string()], &[], &run, &|| None).unwrap();
        assert!(!out[0].ok && mutating(&calls).is_empty());
    }

    #[test]
    fn unprotected_volume_goes_without_confirmation() {
        let calls = RefCell::new(Vec::new());
        let run = recording(&calls, "<no value>");
        let out = clean_with(&["volume:scratch".to_string()], &[], &run, &|| None).unwrap();
        assert!(out[0].ok);
    }

    #[test]
    fn a_failing_removal_is_reported_and_the_rest_continue() {
        let run = |args: &[&str]| -> Result<String, String> {
            match args[0] {
                "system" => Err("no df".into()),
                "rm" => Err("container is running".into()),
                _ => Ok(String::new()),
            }
        };
        let ids = vec![format!("container:{C1}"), "network:a9ae4e7f62e2".to_string()];
        let out = clean_with(&ids, &[], &run, &|| None).unwrap();
        assert!(!out[0].ok && out[0].message.contains("running"));
        assert!(out[1].ok);
    }

    #[test]
    fn a_data_root_inside_docker_desktops_vm_is_not_measured_here() {
        let run = |_: &[&str]| -> Result<String, String> { Ok("/var/lib/docker\n".to_string()) };
        // A Linux path is a folder on Linux, but on Windows it names nothing.
        assert_eq!(data_root(&run).is_some(), cfg!(unix));
        let empty = |_: &[&str]| -> Result<String, String> { Ok("\n".to_string()) };
        assert_eq!(data_root(&empty), None);
    }

    #[test]
    fn a_malformed_id_runs_nothing() {
        let calls = RefCell::new(Vec::new());
        let run = recording(&calls, "");
        let ids = vec![format!("container:{C1}"), "container:x; rm -rf /".to_string()];
        assert!(clean_with(&ids, &[], &run, &|| None).is_err());
        assert!(calls.borrow().is_empty());
    }
}
