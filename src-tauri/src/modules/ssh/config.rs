//! The `IdentityFile` lines of `~/.ssh/config` that apply to a host, so the
//! ssh2 side (SFTP, key install) offers the same key files the terminal's
//! real `ssh` does. Without this a host set up only in the config file logs
//! in from the terminal and fails in the file browser.
//!
//! Only what's needed to pick key files is read: `Host` blocks (with `*`, `?`
//! and `!` patterns), `Match all` / `Match host …`, `Include`, and
//! `IdentityFile` itself with the `~`, `%d`, `%h`, `%p`, `%r` and `%%`
//! expansions. Any other `Match` criterion is treated as not matching:
//! reading a key file the user didn't mean for this host only costs an
//! attempt, but guessing at `exec` or `localnetwork` isn't worth it.

use std::path::{Path, PathBuf};

/// Includes nested deeper than this are ignored, as a loop guard (OpenSSH uses 16).
const MAX_INCLUDE_DEPTH: usize = 16;

/// The host a lookup is for: `host` as the user typed it, which is what
/// OpenSSH matches `Host` patterns against.
pub struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
}

/// The key files the config names for `target`, in file order, duplicates
/// dropped. The user's `~/.ssh/config` is read first, then the system-wide
/// file, as `ssh` does.
pub fn identity_files(target: &Target) -> Vec<PathBuf> {
    let Ok(home) = super::home_dir() else { return Vec::new() };
    let ssh_dir = home.join(".ssh");
    let mut out = Vec::new();
    for file in [ssh_dir.join("config"), system_config()] {
        if let Ok(text) = std::fs::read_to_string(&file) {
            collect(&text, target, &home, &ssh_dir, 0, &mut out);
        }
    }
    let mut seen = Vec::new();
    out.retain(|p| {
        let key = normalize(p);
        let fresh = !seen.contains(&key);
        seen.push(key);
        fresh
    });
    out
}

#[cfg(windows)]
fn system_config() -> PathBuf {
    let data = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    data.join("ssh").join("ssh_config")
}

#[cfg(not(windows))]
fn system_config() -> PathBuf {
    PathBuf::from("/etc/ssh/ssh_config")
}

/// A path in the form two spellings of one file compare equal in: Windows
/// paths are case-insensitive and take either slash.
pub fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s.into_owned()
    }
}

/// Walks one config file's text. `active` starts true: lines before the
/// first `Host` apply to every host. An `Include` inside a block that doesn't
/// apply is skipped, and a `Host` line inside an included file only governs
/// the rest of that file.
fn collect(text: &str, target: &Target, home: &Path, ssh_dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let mut active = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((keyword, rest)) = split_keyword(line) else { continue };
        let args = tokenize(rest);
        match keyword.to_ascii_lowercase().as_str() {
            "host" => active = host_matches(&args, target.host),
            "match" => active = match_applies(&args, target.host),
            "include" if active && depth < MAX_INCLUDE_DEPTH => {
                for arg in &args {
                    for file in include_files(arg, home, ssh_dir) {
                        if let Ok(text) = std::fs::read_to_string(&file) {
                            collect(&text, target, home, ssh_dir, depth + 1, out);
                        }
                    }
                }
            }
            "identityfile" if active => {
                if let Some(arg) = args.first() {
                    if !arg.eq_ignore_ascii_case("none") {
                        out.push(expand(arg, target, home));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `Keyword value` or `Keyword=value`, the keyword case-insensitive.
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let end = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let (keyword, rest) = line.split_at(end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    Some((keyword, rest))
}

/// Whitespace-separated arguments, a double-quoted one kept whole (a key path
/// under `C:\Users\First Last` has to be quoted).
fn tokenize(rest: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has = false;
    for c in rest.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    args.push(std::mem::take(&mut current));
                    has = false;
                }
            }
            c => {
                current.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(current);
    }
    args
}

/// `Host` semantics: true when some pattern matches and no `!pattern` does.
fn host_matches(patterns: &[String], host: &str) -> bool {
    let mut matched = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard(negated, host) {
                return false;
            }
        } else if wildcard(pattern, host) {
            matched = true;
        }
    }
    matched
}

/// `Match all` and `Match host a,b,!c`, every criterion having to hold;
/// anything else is taken not to match (see the module note).
fn match_applies(args: &[String], host: &str) -> bool {
    let mut i = 0;
    let mut any = false;
    while i < args.len() {
        match args[i].to_ascii_lowercase().as_str() {
            "all" => i += 1,
            "host" if i + 1 < args.len() => {
                let list: Vec<String> = args[i + 1].split(',').map(str::to_string).collect();
                if !host_matches(&list, host) {
                    return false;
                }
                i += 2;
            }
            _ => return false,
        }
        any = true;
    }
    any
}

/// Glob match with `*` and `?`, case-insensitive like OpenSSH's host matching.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// `~` and the `%` tokens OpenSSH allows in `IdentityFile`. A relative result
/// is taken from the home directory, as `ssh` does for the user's config.
fn expand(arg: &str, target: &Target, home: &Path) -> PathBuf {
    let home_str = home.to_string_lossy();
    let mut s = String::new();
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            s.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => s.push('%'),
            Some('d') => s.push_str(&home_str),
            Some('h') => s.push_str(target.host),
            Some('p') => s.push_str(&target.port.to_string()),
            Some('r') => s.push_str(target.user),
            Some(other) => {
                s.push('%');
                s.push(other);
            }
            None => s.push('%'),
        }
    }
    let path = if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        let p = PathBuf::from(&s);
        if p.has_root() {
            p
        } else {
            home.join(p)
        }
    };
    tidy(path)
}

/// On Windows, one kind of slash, so the path reads (and compares) like the
/// ones the key list reports.
fn tidy(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().replace('/', "\\"))
    } else {
        path
    }
}

/// The files one `Include` argument names: relative to `~/.ssh`, `~`
/// expanded, and `*`/`?` in the last component matched against the folder,
/// in name order as `glob` returns them.
fn include_files(arg: &str, home: &Path, ssh_dir: &Path) -> Vec<PathBuf> {
    let path = if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        let p = PathBuf::from(arg);
        if p.has_root() {
            p
        } else {
            ssh_dir.join(p)
        }
    };
    let path = tidy(path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| wildcard(&name, &n.to_string_lossy())))
        .collect();
    files.sort();
    files
}

/// For the host form: the key files `~/.ssh/config` gives this host, so the
/// form can tell "this key will be used" from "this key is ignored".
#[tauri::command(async)]
pub fn ssh_config_identities(host: String, port: u16, username: String) -> Vec<String> {
    identity_files(&Target { host: host.trim(), port, user: username.trim() })
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target<'a>(host: &'a str) -> Target<'a> {
        Target { host, port: 22, user: "deploy" }
    }

    fn run(text: &str, host: &str) -> Vec<PathBuf> {
        let home = Path::new("/home/me");
        let mut out = Vec::new();
        collect(text, &target(host), home, &home.join(".ssh"), 0, &mut out);
        out
    }

    fn p(s: &str) -> PathBuf {
        tidy(PathBuf::from(s))
    }

    #[test]
    fn picks_the_identity_of_the_matching_host_block() {
        let config = "Host 52.62.3.173\n    User ploi\n    IdentityFile ~/.ssh/dealercore-server\n\nHost other\n  IdentityFile ~/.ssh/other\n";
        assert_eq!(run(config, "52.62.3.173"), [p("/home/me/.ssh/dealercore-server")]);
        assert_eq!(run(config, "other"), [p("/home/me/.ssh/other")]);
        assert!(run(config, "elsewhere").is_empty());
    }

    #[test]
    fn host_patterns_wildcards_negation_and_case() {
        let config = "Host *.example.com !bastion.example.com\n IdentityFile=~/.ssh/work\nhost DB?\n IDENTITYFILE ~/.ssh/db\n";
        assert_eq!(run(config, "api.example.com"), [p("/home/me/.ssh/work")]);
        assert!(run(config, "bastion.example.com").is_empty());
        assert_eq!(run(config, "db1"), [p("/home/me/.ssh/db")]);
        assert!(run(config, "db12").is_empty());
    }

    #[test]
    fn global_lines_and_star_blocks_apply_to_every_host() {
        let config = "IdentityFile ~/.ssh/top\nHost *\n  IdentityFile ~/.ssh/all\nHost x\n  IdentityFile none\n";
        assert_eq!(run(config, "x"), [p("/home/me/.ssh/top"), p("/home/me/.ssh/all")]);
    }

    #[test]
    fn expands_tokens_quotes_and_relative_paths() {
        let config = "Host h\n IdentityFile \"%d/keys dir/%r@%h_%p\"\n IdentityFile keys/rel\n IdentityFile %d/100%%\n";
        assert_eq!(
            run(config, "h"),
            [p("/home/me/keys dir/deploy@h_22"), p("/home/me/keys/rel"), p("/home/me/100%")]
        );
    }

    #[test]
    fn match_all_and_match_host_are_understood_and_others_are_not() {
        let config = "Match host a,b\n IdentityFile ~/k1\nMatch all\n IdentityFile ~/k2\nMatch exec true\n IdentityFile ~/k3\n";
        assert_eq!(run(config, "a"), [p("/home/me/k1"), p("/home/me/k2")]);
        assert_eq!(run(config, "c"), [p("/home/me/k2")]);
    }

    #[test]
    fn includes_are_followed_relative_to_the_ssh_dir() {
        let dir = std::env::temp_dir().join(format!("devspro-sshcfg-{}", uuid::Uuid::new_v4()));
        let ssh = dir.join(".ssh");
        std::fs::create_dir_all(ssh.join("conf.d")).unwrap();
        std::fs::write(ssh.join("conf.d").join("a.conf"), "Host inc\n IdentityFile ~/.ssh/from-include\n").unwrap();
        std::fs::write(ssh.join("conf.d").join("skip.txt"), "IdentityFile ~/.ssh/wrong\n").unwrap();
        let mut out = Vec::new();
        collect("Include conf.d/*.conf\nHost never\n Include conf.d/skip.txt\n", &target("inc"), &dir, &ssh, 0, &mut out);
        assert_eq!(out, [tidy(dir.join(".ssh/from-include"))]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wildcard_matches_like_a_glob() {
        assert!(wildcard("*", ""));
        assert!(wildcard("a*c", "abbbc"));
        assert!(wildcard("a?c", "abc"));
        assert!(!wildcard("a?c", "ac"));
        assert!(wildcard("*.Example.com", "x.example.COM"));
    }
}
