// Deliberately has zero dependency on the main app crate — this binary is the one thing
// that runs elevated, so it stays small and independently auditable. Its manifest schema
// is a plain copy of `elevate::Operation` in the main crate, kept in sync by hand.
mod platform;

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Operation {
    Ping,
    WriteFile { path: String, contents: String },
    RemoveFile { path: String },
    SyncMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
        block_lines: Vec<String>,
    },
    RemoveMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
    },
    EnsureNginxConfDInclude { conf_d_dir: String, sites_glob: String },
    EnsureIncludeInHttpBlock {
        config_path: String,
        marker_begin: String,
        marker_end: String,
        include_line: String,
    },
    RunNginxTest {
        binary_path: String,
        prefix: Option<String>,
    },
    ReloadNginx {
        binary_path: String,
        prefix: Option<String>,
    },
    InstallRootCa {
        cert_path: String,
        nss_db_paths: Vec<String>,
    },
    InstallPackage { manager: String, packages: Vec<String> },
    FixLaravelPermissions { project_path: String },
}

#[derive(Debug, Deserialize)]
struct Manifest {
    allowed_write_prefix: String,
    operations: Vec<Operation>,
}

#[derive(Debug, Serialize)]
struct OpResult {
    success: bool,
    message: String,
}

fn is_within_prefix(prefix: &str, path: &str) -> bool {
    if prefix.is_empty() || path.contains("..") {
        return false;
    }
    Path::new(path).starts_with(Path::new(prefix))
}

/// Writes via a scratch file in the same directory and renames over the target, so a
/// reader never sees a half-written `/etc/hosts` or nginx.conf.
///
/// The scratch name is unique per *process*, not per call — which is sound only because a
/// manifest's operations are applied strictly one at a time (see `main`), and each helper
/// invocation is its own process. Two concurrent calls within one process would stage
/// through the same path and rename over each other; the tests below have to give every
/// case its own directory for exactly that reason.
fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = dir.join(format!(".devsuite-tmp-{}", std::process::id()));
    fs::write(&tmp_path, contents)?;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp_path, meta.permissions());
    }
    fs::rename(&tmp_path, path)
}

fn write_file_checked(allowed_prefix: &str, path: &str, contents: &str) -> OpResult {
    if !is_within_prefix(allowed_prefix, path) {
        return OpResult {
            success: false,
            message: format!("{path} is outside the allowed prefix ({allowed_prefix}) — refusing to write"),
        };
    }
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return OpResult {
                success: false,
                message: format!("failed to create parent dir for {path}: {e}"),
            };
        }
    }
    match fs::write(p, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {path}: {e}"),
        },
    }
}

fn remove_file_checked(allowed_prefix: &str, path: &str) -> OpResult {
    if !is_within_prefix(allowed_prefix, path) {
        return OpResult {
            success: false,
            message: format!("{path} is outside the allowed prefix ({allowed_prefix}) — refusing to remove"),
        };
    }
    match fs::remove_file(path) {
        Ok(()) => OpResult {
            success: true,
            message: format!("removed {path}"),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => OpResult {
            success: true,
            message: format!("{path} already absent"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to remove {path}: {e}"),
        },
    }
}

fn sync_marked_block(file_path: &str, marker_begin: &str, marker_end: &str, block_lines: &[String]) -> OpResult {
    let path = Path::new(file_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {file_path}: {e}"),
            }
        }
    };

    let mut new_block = String::new();
    new_block.push_str(marker_begin);
    new_block.push('\n');
    for line in block_lines {
        new_block.push_str(line);
        new_block.push('\n');
    }
    new_block.push_str(marker_end);
    new_block.push('\n');

    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let new_content = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if e >= b => {
            let mut out = String::new();
            for l in &lines[..b] {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str(&new_block);
            for l in &lines[e + 1..] {
                out.push_str(l);
                out.push('\n');
            }
            out
        }
        _ => {
            let mut out = original.clone();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(&new_block);
            out
        }
    };

    match atomic_write(path, &new_content) {
        Ok(()) => OpResult {
            success: true,
            message: format!("synced marked block in {file_path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {file_path}: {e}"),
        },
    }
}

fn remove_marked_block(file_path: &str, marker_begin: &str, marker_end: &str) -> OpResult {
    let path = Path::new(file_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {file_path}: {e}"),
            }
        }
    };

    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let (b, e) = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if e >= b => (b, e),
        _ => {
            return OpResult {
                success: true,
                message: format!("no marked block found in {file_path}, nothing to remove"),
            }
        }
    };

    let mut out = String::new();
    for l in lines.iter().take(b) {
        out.push_str(l);
        out.push('\n');
    }
    for l in lines.iter().skip(e + 1) {
        out.push_str(l);
        out.push('\n');
    }

    match atomic_write(path, &out) {
        Ok(()) => OpResult {
            success: true,
            message: format!("removed marked block from {file_path}"),
        },
        Err(err) => OpResult {
            success: false,
            message: format!("failed to write {file_path}: {err}"),
        },
    }
}

fn ensure_nginx_confd_include(conf_d_dir: &str, sites_glob: &str) -> OpResult {
    let dir = Path::new(conf_d_dir);
    if !dir.is_dir() {
        return OpResult {
            success: false,
            message: format!("{conf_d_dir} is not a directory"),
        };
    }
    let dest = dir.join("devsuite.conf");
    let contents = format!("include {sites_glob};\n");
    match fs::write(&dest, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {}", dest.display()),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {}: {e}", dest.display()),
        },
    }
}

/// Splices one marker-wrapped `include` line in immediately after the line that opens
/// `http{}`, replacing the block in place if it's already there.
///
/// Where it goes is the whole reason this exists rather than reusing `sync_marked_block`:
/// that one appends to the end of the file when the markers are absent, which for
/// nginx.conf means *after* `http{}` closes — where an `include` of `server{}` blocks is a
/// syntax error. `sites` on Linux avoids the problem entirely by dropping a file into
/// `conf.d`, but the stock Windows nginx has no such directory and includes nothing.
///
/// Finding `http{}` by scanning for the opening line is enough for a real nginx.conf: the
/// directive is `http` followed by `{`, at most one per file, and never inside a comment
/// in any distribution's shipped config. A file with no `http` block at all is reported as
/// an error rather than guessed at, since inserting anywhere else would produce a config
/// that fails `nginx -t` with a message pointing at our own line.
fn ensure_include_in_http_block(
    config_path: &str,
    marker_begin: &str,
    marker_end: &str,
    include_line: &str,
) -> OpResult {
    let path = Path::new(config_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {config_path}: {e}"),
            }
        }
    };

    let block = format!("{marker_begin}\n{include_line}\n{marker_end}\n");
    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let (before, after) = match (begin_idx, end_idx) {
        // Already present — replace exactly the marked span, leaving it where it is.
        (Some(b), Some(e)) if e >= b => (&lines[..b], &lines[e + 1..]),
        _ => match lines.iter().position(is_http_block_opening) {
            Some(http) => (&lines[..=http], &lines[http + 1..]),
            None => {
                return OpResult {
                    success: false,
                    message: format!(
                        "{config_path} has no `http {{` block to add the include to — this nginx \
                         configuration doesn't use the standard layout, so automatic site setup \
                         isn't supported for it"
                    ),
                }
            }
        },
    };

    let mut out = String::new();
    for line in before {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&block);
    for line in after {
        out.push_str(line);
        out.push('\n');
    }

    match atomic_write(path, &out) {
        Ok(()) => OpResult {
            success: true,
            message: format!("ensured DevsPro include inside http{{}} in {config_path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {config_path}: {e}"),
        },
    }
}

/// The line that opens nginx's `http` block: the bare directive, then `{` either on the
/// same line or (legally, if unusually) alone on the next one. A commented-out `http {`
/// must not count, or the include lands inside a comment and silently does nothing.
fn is_http_block_opening(line: &&str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return false;
    }
    let without_brace = trimmed.strip_suffix('{').unwrap_or(trimmed).trim_end();
    without_brace == "http"
}

/// `prefix` is nginx's `-p`, and is `None` on Unix — see `Operation::RunNginxTest` in the
/// main crate for why Windows cannot leave it out.
fn run_nginx_test(binary_path: &str, prefix: Option<&str>) -> OpResult {
    let mut command = Command::new(binary_path);
    if let Some(prefix) = prefix {
        command.args(["-p", prefix]);
    }
    match command.arg("-t").output() {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            OpResult {
                success: out.status.success(),
                message: combined.trim().to_string(),
            }
        }
        Err(e) => OpResult {
            success: false,
            message: format!("failed to run `{binary_path} -t`: {e}"),
        },
    }
}

fn apply(allowed_prefix: &str, op: Operation) -> OpResult {
    match op {
        Operation::Ping => OpResult {
            success: true,
            message: "pong from ldm-helper".to_string(),
        },
        Operation::WriteFile { path, contents } => write_file_checked(allowed_prefix, &path, &contents),
        Operation::RemoveFile { path } => remove_file_checked(allowed_prefix, &path),
        Operation::SyncMarkedBlock {
            file_path,
            marker_begin,
            marker_end,
            block_lines,
        } => sync_marked_block(&file_path, &marker_begin, &marker_end, &block_lines),
        Operation::RemoveMarkedBlock {
            file_path,
            marker_begin,
            marker_end,
        } => remove_marked_block(&file_path, &marker_begin, &marker_end),
        Operation::EnsureNginxConfDInclude { conf_d_dir, sites_glob } => {
            ensure_nginx_confd_include(&conf_d_dir, &sites_glob)
        }
        Operation::EnsureIncludeInHttpBlock {
            config_path,
            marker_begin,
            marker_end,
            include_line,
        } => ensure_include_in_http_block(&config_path, &marker_begin, &marker_end, &include_line),
        Operation::RunNginxTest { binary_path, prefix } => {
            run_nginx_test(&binary_path, prefix.as_deref())
        }
        Operation::ReloadNginx { binary_path, prefix } => {
            platform::reload_nginx(&binary_path, prefix.as_deref())
        }
        // The prefix check stays here, not in `platform`: it is the security boundary this
        // binary exists to enforce, and it is the same on every OS. Only what "install into
        // the trust store" means is platform-specific.
        Operation::InstallRootCa { cert_path, nss_db_paths } => {
            if !is_within_prefix(allowed_prefix, &cert_path) {
                OpResult {
                    success: false,
                    message: format!("{cert_path} is outside the allowed prefix — refusing to install"),
                }
            } else {
                platform::install_root_ca(&cert_path, &nss_db_paths)
            }
        }
        Operation::InstallPackage { manager, packages } => {
            if packages.is_empty() {
                OpResult {
                    success: false,
                    message: "no packages specified".to_string(),
                }
            } else {
                platform::install_package(&manager, &packages)
            }
        }
        Operation::FixLaravelPermissions { project_path } => {
            platform::fix_laravel_permissions(&project_path)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(manifest_path) = args.get(1) else {
        eprintln!("usage: ldm-helper <manifest-path>");
        std::process::exit(2);
    };

    let manifest_path = PathBuf::from(manifest_path);
    let result_path = manifest_path.with_extension("result.json");

    let results: Vec<OpResult> = match fs::read_to_string(&manifest_path) {
        Ok(raw) => match serde_json::from_str::<Manifest>(&raw) {
            Ok(manifest) => {
                // Abort-on-first-failure within one manifest: a later op (e.g.
                // ReloadNginx) never runs if an earlier one (e.g. RunNginxTest)
                // failed, so a broken config can't get reloaded. Already-applied
                // steps are not automatically rolled back in this version.
                let prefix = manifest.allowed_write_prefix.clone();
                let mut results = Vec::new();
                let mut aborted = false;
                for op in manifest.operations {
                    if aborted {
                        results.push(OpResult {
                            success: false,
                            message: "skipped: an earlier operation in this manifest failed".to_string(),
                        });
                        continue;
                    }
                    let r = apply(&prefix, op);
                    if !r.success {
                        aborted = true;
                    }
                    results.push(r);
                }
                results
            }
            Err(e) => vec![OpResult {
                success: false,
                message: format!("invalid manifest: {e}"),
            }],
        },
        Err(e) => vec![OpResult {
            success: false,
            message: format!("could not read manifest: {e}"),
        }],
    };

    let out = serde_json::to_string_pretty(&results).unwrap_or_default();
    if let Err(e) = fs::write(&result_path, out) {
        eprintln!("failed to write result file: {e}");
        std::process::exit(1);
    }
    // Exit 0 regardless of per-op outcomes — the caller reads success/failure per
    // operation from the result file. A non-zero exit here is reserved for the
    // helper itself failing to run (unreadable manifest, unwritable result file),
    // which platform::launch_helper treats as "the elevated call itself failed."
}

/// Everything covered here is the part of this binary that edits files while running as
/// root. It had no tests at all before the Windows port needed a third block-editing
/// function alongside the two that already existed — which is exactly the wrong number of
/// similar-looking functions to leave unguarded in the one component that can overwrite
/// `/etc/hosts` and the user's real nginx.conf.
#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded file in a directory of its own.
    ///
    /// The directory matters: `atomic_write` names its scratch file after the process id
    /// alone, so every test in this binary would otherwise stage through the same
    /// `.devsuite-tmp-<pid>` path and rename it over each other's target — tests failing
    /// with another test's contents. That is safe in the helper itself, which applies a
    /// manifest's operations strictly one at a time in a process of its own, but it is not
    /// safe for a parallel test runner.
    struct TempDirFile {
        dir: PathBuf,
        file: PathBuf,
    }

    impl TempDirFile {
        fn with(contents: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("ldm-helper-test-{}-{seq}", std::process::id()));
            fs::create_dir_all(&dir).expect("failed to create test dir");
            let file = dir.join("target.conf");
            fs::write(&file, contents).expect("failed to seed test file");
            TempDirFile { dir, file }
        }

        fn path(&self) -> &str {
            self.file.to_str().expect("temp path is not valid UTF-8")
        }

        fn read(&self) -> String {
            fs::read_to_string(&self.file).expect("failed to read test file")
        }
    }

    impl Drop for TempDirFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    const BEGIN: &str = "# >>> devsuite >>>";
    const END: &str = "# <<< devsuite <<<";

    // --- the allowed-prefix guard -------------------------------------------------

    #[test]
    fn prefix_guard_admits_only_paths_under_the_declared_prefix() {
        assert!(is_within_prefix("/home/me/.local/share/DevsPro", "/home/me/.local/share/DevsPro/nginx/sites/a.conf"));
        assert!(!is_within_prefix("/home/me/.local/share/DevsPro", "/etc/passwd"));
        // A neighbouring directory that merely shares a name prefix is still outside.
        assert!(!is_within_prefix("/home/me/DevsPro", "/home/me/DevsPro-evil/x.conf"));
    }

    #[test]
    fn prefix_guard_rejects_traversal_and_an_empty_prefix() {
        // Traversal is refused on the raw string, before any canonicalisation could be
        // fooled by a symlink under the prefix.
        assert!(!is_within_prefix("/home/me/DevsPro", "/home/me/DevsPro/../../../etc/passwd"));
        // An empty prefix would otherwise match every path — `Ping` and
        // `FixLaravelPermissions` legitimately send one, so this must never be
        // interpreted as "anywhere".
        assert!(!is_within_prefix("", "/anything"));
    }

    // --- SyncMarkedBlock ---------------------------------------------------------

    #[test]
    fn sync_replaces_an_existing_block_in_place_and_leaves_the_rest_alone() {
        let file = TempDirFile::with(&format!(
            "127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 old.test\n{END}\n# trailing comment\n"
        ));
        let result = sync_marked_block(
            file.path(),
            BEGIN,
            END,
            &["127.0.0.1 new.test".to_string()],
        );
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 new.test\n{END}\n# trailing comment\n")
        );
    }

    #[test]
    fn sync_appends_the_block_when_the_markers_are_absent() {
        let file = TempDirFile::with("127.0.0.1 localhost\n");
        let result = sync_marked_block(file.path(), BEGIN, END, &["127.0.0.1 a.test".to_string()]);
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("127.0.0.1 localhost\n\n{BEGIN}\n127.0.0.1 a.test\n{END}\n")
        );
    }

    /// An empty set of hostnames is the normal state after removing the last site, and it
    /// has to leave an empty block behind rather than deleting the markers — the next add
    /// finds and replaces them instead of appending a second copy.
    #[test]
    fn sync_with_no_lines_keeps_an_empty_marked_block() {
        let file = TempDirFile::with(&format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 a.test\n{END}\n"));
        let result = sync_marked_block(file.path(), BEGIN, END, &[]);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), format!("127.0.0.1 localhost\n{BEGIN}\n{END}\n"));
    }

    // --- RemoveMarkedBlock -------------------------------------------------------

    #[test]
    fn remove_takes_the_markers_with_it() {
        let file = TempDirFile::with(&format!("before\n{BEGIN}\ninclude x;\n{END}\nafter\n"));
        let result = remove_marked_block(file.path(), BEGIN, END);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), "before\nafter\n");
    }

    /// Called unconditionally on every add and remove, so the no-block case is the common
    /// one and must be a successful no-op rather than an error that aborts the manifest.
    #[test]
    fn remove_is_a_successful_no_op_when_there_is_no_block() {
        let file = TempDirFile::with("http {\n}\n");
        let result = remove_marked_block(file.path(), BEGIN, END);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), "http {\n}\n");
    }

    // --- EnsureIncludeInHttpBlock ------------------------------------------------

    #[test]
    fn include_goes_immediately_inside_the_http_block() {
        let file = TempDirFile::with("events {\n}\n\nhttp {\n    include mime.types;\n}\n");
        let result = ensure_include_in_http_block(file.path(), BEGIN, END, "include sites/*.conf;");
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("events {{\n}}\n\nhttp {{\n{BEGIN}\ninclude sites/*.conf;\n{END}\n    include mime.types;\n}}\n")
        );
    }

    /// Re-applied on every add, enable and remove, so running it repeatedly has to be
    /// indistinguishable from running it once.
    #[test]
    fn include_is_idempotent_and_updates_in_place() {
        let file = TempDirFile::with("http {\n    include mime.types;\n}\n");
        for line in ["include first/*.conf;", "include first/*.conf;", "include second/*.conf;"] {
            let result = ensure_include_in_http_block(file.path(), BEGIN, END, line);
            assert!(result.success, "{}", result.message);
        }
        assert_eq!(
            file.read(),
            format!("http {{\n{BEGIN}\ninclude second/*.conf;\n{END}\n    include mime.types;\n}}\n")
        );
        assert_eq!(file.read().matches(BEGIN).count(), 1, "exactly one block, never stacked");
    }

    /// The failure this whole operation exists to avoid: appended at the end of the file
    /// the include lands after `http{}` closes, where nginx rejects a `server{}` block. So
    /// a config with no `http{}` must be refused, not appended to.
    #[test]
    fn include_refuses_a_config_with_no_http_block() {
        let file = TempDirFile::with("events {\n    worker_connections 1024;\n}\n");
        let result = ensure_include_in_http_block(file.path(), BEGIN, END, "include sites/*.conf;");
        assert!(!result.success);
        assert!(result.message.contains("no `http {` block"), "{}", result.message);
        assert_eq!(file.read(), "events {\n    worker_connections 1024;\n}\n", "left untouched");
    }

    #[test]
    fn http_block_opening_ignores_comments_and_lookalikes() {
        assert!(is_http_block_opening(&"http {"));
        assert!(is_http_block_opening(&"  http{"));
        // `{` on its own line is legal nginx, and the marker still belongs after `http`.
        assert!(is_http_block_opening(&"http"));
        // A commented-out block would swallow the include into the comment.
        assert!(!is_http_block_opening(&"# http {"));
        // Other blocks and directives that merely start with the same letters.
        assert!(!is_http_block_opening(&"https {"));
        assert!(!is_http_block_opening(&"http2 on;"));
        assert!(!is_http_block_opening(&"    server {"));
    }
}
