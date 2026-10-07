//! Which running processes hold a cache open. A cache whose owner is running
//! is reported as blocked rather than cleaned underneath it.

use std::fs;

/// One running process: its short name and argument list.
#[derive(Debug, Clone, Default)]
pub struct Proc {
    pub comm: String,
    pub args: Vec<String>,
}

/// Reads /proc once. Unreadable entries (other users, exited meanwhile) are skipped.
pub fn snapshot() -> Vec<Proc> {
    let Ok(dir) = fs::read_dir("/proc") else { return Vec::new() };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let p = entry.path();
        let Ok(comm) = fs::read_to_string(p.join("comm")) else { continue };
        let args = fs::read(p.join("cmdline"))
            .map(|raw| {
                raw.split(|b| *b == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        out.push(Proc { comm: comm.trim().to_string(), args });
    }
    out
}

/// What can keep a cache from being cleaned right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    /// A process whose name (`comm`, truncated to 15 bytes by the kernel) is one of these.
    Comm(&'static str, &'static [&'static str]),
    /// A gradle daemon: a `java` process running the daemon main class.
    Gradle,
    /// npm / pnpm / yarn / bun in the middle of an install.
    Installing(&'static str),
}

const INSTALL_VERBS: &[&str] = &["install", "i", "ci", "add", "update", "upgrade", "up"];

fn installing(manager: &str, p: &Proc) -> bool {
    let runner = matches!(p.comm.as_str(), "node" | "bun") || p.comm == manager;
    if !runner {
        return false;
    }
    let names_manager = p.args.iter().any(|a| {
        let file = a.rsplit('/').next().unwrap_or(a);
        file == manager || file.starts_with(&format!("{manager}-")) || file.starts_with(&format!("{manager}."))
    });
    names_manager && p.args.iter().any(|a| INSTALL_VERBS.contains(&a.as_str()))
}

impl Blocker {
    /// A sentence for the table, or None when nothing matching is running.
    pub fn hit(&self, procs: &[Proc]) -> Option<String> {
        match self {
            Blocker::Comm(label, comms) => procs
                .iter()
                .any(|p| comms.contains(&p.comm.as_str()))
                .then(|| format!("{label} is running")),
            Blocker::Gradle => procs
                .iter()
                .any(|p| {
                    p.comm == "java"
                        && p.args.iter().any(|a| a.contains("GradleDaemon") || a.contains("gradle.launcher.daemon"))
                })
                .then(|| "a Gradle daemon is running".to_string()),
            Blocker::Installing(manager) => procs
                .iter()
                .any(|p| installing(manager, p))
                .then(|| format!("{manager} is installing")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(comm: &str, args: &[&str]) -> Proc {
        Proc { comm: comm.into(), args: args.iter().map(|a| a.to_string()).collect() }
    }

    #[test]
    fn matches_a_browser_by_comm() {
        let b = Blocker::Comm("Firefox", &["firefox", "firefox-bin"]);
        assert_eq!(b.hit(&[proc("bash", &[]), proc("firefox", &[])]).as_deref(), Some("Firefox is running"));
        assert!(b.hit(&[proc("bash", &[])]).is_none());
    }

    #[test]
    fn finds_a_gradle_daemon_among_java_processes() {
        let daemon = proc("java", &["java", "-cp", "x", "org.gradle.launcher.daemon.bootstrap.GradleDaemon"]);
        assert!(Blocker::Gradle.hit(&[proc("java", &["java", "-jar", "app.jar"])]).is_none());
        assert!(Blocker::Gradle.hit(&[daemon]).is_some());
    }

    #[test]
    fn installing_needs_the_manager_and_an_install_verb() {
        let install = proc("node", &["node", "/usr/lib/node_modules/npm/bin/npm-cli.js", "install"]);
        let run = proc("node", &["node", "/usr/lib/node_modules/npm/bin/npm-cli.js", "run", "dev"]);
        assert!(Blocker::Installing("npm").hit(&[install.clone()]).is_some());
        assert!(Blocker::Installing("npm").hit(&[run]).is_none());
        assert!(Blocker::Installing("pnpm").hit(&[install]).is_none());
        assert!(Blocker::Installing("yarn").hit(&[proc("yarn", &["yarn", "add", "left-pad"])]).is_some());
    }
}
