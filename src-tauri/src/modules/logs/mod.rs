//! Aggregated, read-only log viewer. Owns no files and no DB table — it just
//! surfaces logs other modules already produce: nginx's per-site access/error
//! logs (`nginx_ssl::logs_dir`) and, for Laravel-backed sites, the app's own
//! `storage/logs/laravel.log`.
//!
//! The frontend never sends a raw filesystem path. `logs_tail` takes an opaque
//! `source_id` and re-derives the allowed path set server-side, so it can only
//! ever read a file `logs_list_sources` itself would have offered.

use super::nginx_ssl::sites::{self, Backend, Site};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize)]
pub struct LogSource {
    pub id: String,
    pub site_name: String,
    pub hostname: String,
    pub kind: String,
    pub path: String,
}

fn sources_for_site(logs_dir: &std::path::Path, site: &Site) -> Vec<LogSource> {
    let mut sources = Vec::new();

    let access_log = logs_dir.join(format!("{}.access.log", site.slug));
    if access_log.exists() {
        sources.push(LogSource {
            id: format!("{}:access", site.id),
            site_name: site.name.clone(),
            hostname: site.hostname.clone(),
            kind: "access".to_string(),
            path: access_log.to_string_lossy().to_string(),
        });
    }

    let error_log = logs_dir.join(format!("{}.error.log", site.slug));
    if error_log.exists() {
        sources.push(LogSource {
            id: format!("{}:error", site.id),
            site_name: site.name.clone(),
            hostname: site.hostname.clone(),
            kind: "error".to_string(),
            path: error_log.to_string_lossy().to_string(),
        });
    }

    if let Backend::Laravel { project_path, .. } = &site.backend {
        let laravel_log = PathBuf::from(project_path).join("storage/logs/laravel.log");
        if laravel_log.exists() {
            sources.push(LogSource {
                id: format!("{}:laravel", site.id),
                site_name: site.name.clone(),
                hostname: site.hostname.clone(),
                kind: "laravel".to_string(),
                path: laravel_log.to_string_lossy().to_string(),
            });
        }
    }

    sources
}

fn list_sources(app: &AppHandle, sites: &[Site]) -> Result<Vec<LogSource>, String> {
    let logs_dir = super::nginx_ssl::logs_dir(app)?;
    Ok(sites.iter().flat_map(|site| sources_for_site(&logs_dir, site)).collect())
}

pub fn tail_lines(contents: &str, n: u32) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(n as usize);
    lines[start..].join("\n")
}

#[tauri::command]
pub fn logs_list_sources(app: AppHandle, db: State<crate::db::Db>) -> Result<Vec<LogSource>, String> {
    let all_sites = sites::nginx_ssl_list_sites(db)?;
    list_sources(&app, &all_sites)
}

#[tauri::command]
pub fn logs_tail(app: AppHandle, db: State<crate::db::Db>, source_id: String, lines: u32) -> Result<String, String> {
    let all_sites = sites::nginx_ssl_list_sites(db)?;
    let source = list_sources(&app, &all_sites)?
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| "unknown log source".to_string())?;
    let contents = std::fs::read_to_string(&source.path).map_err(|e| format!("failed to read log: {e}"))?;
    Ok(tail_lines(&contents, lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_lines_keeps_all_when_fewer_than_n() {
        assert_eq!(tail_lines("a\nb\nc", 10), "a\nb\nc");
    }

    #[test]
    fn tail_lines_keeps_last_n_in_order() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }

    #[test]
    fn tail_lines_exact_count() {
        assert_eq!(tail_lines("a\nb\nc", 3), "a\nb\nc");
    }

    #[test]
    fn tail_lines_zero_returns_empty() {
        assert_eq!(tail_lines("a\nb\nc", 0), "");
    }

    #[test]
    fn tail_lines_empty_contents() {
        assert_eq!(tail_lines("", 5), "");
    }

    #[test]
    fn tail_lines_ignores_trailing_newline() {
        assert_eq!(tail_lines("a\nb\nc\n", 2), "b\nc");
    }
}
