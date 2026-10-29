//! Ending a task's runs that are still going.
//!
//! Disabling or deleting a scheduled task only changes what fires next; a run
//! already started keeps going. That matters for a command that never exits on
//! its own (`php artisan queue:work` as a "task"), which would otherwise outlive
//! its definition with nothing left in DevsPro to stop it.
//!
//! Nothing here holds a pid: a run may have been started by the scheduler, or by
//! "Run now" before an app restart. What every run does carry is its log path —
//! on the command line (Windows) or as its stdout (Linux) — and the id is a
//! UUID, so `scheduled-logs/<id>.log` names one task's runs and nothing else.
//! Only the run's own wrapper processes are matched, so an editor that happens
//! to have the log open is never touched.
//!
//! Other Unix systems have no `/proc` to find runs by and are left as before:
//! cron's runs there still finish on their own.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::stop_runs;
#[cfg(windows)]
pub use windows::stop_runs;

struct Entry {
    pid: u32,
    parent: u32,
    matches: bool,
}

/// Matching processes whose parent doesn't match too: the wrappers of one run
/// all carry the marker, and stopping the outermost covers the rest.
fn roots(procs: &[Entry]) -> Vec<u32> {
    procs
        .iter()
        .filter(|p| p.matches)
        .filter(|p| !procs.iter().any(|q| q.matches && q.pid == p.parent))
        .map(|p| p.pid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(pid: u32, parent: u32, matches: bool) -> Entry {
        Entry { pid, parent, matches }
    }

    #[test]
    fn a_nested_match_is_covered_by_its_parents_tree_kill() {
        // svchost(1) -> conhost(10, match) -> cmd(11, match) -> php(12)
        // devspro(2) -> cmd(20, match) -> php(21); unrelated cmd(30)
        let procs = [e(1, 0, false), e(10, 1, true), e(11, 10, true), e(12, 11, false), e(20, 2, true), e(21, 20, false), e(30, 2, false)];
        assert_eq!(roots(&procs), vec![10, 20]);
    }
}
