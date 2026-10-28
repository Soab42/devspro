//! Reads an Apache configuration the way httpd itself walks it — following `Include`s,
//! expanding `Define`d variables — closely enough to answer the two questions the
//! generated config depends on:
//!
//! - **Which ports already have a `Listen`.** DevsPro's vhosts need 80 and 443, but a
//!   second `Listen 443` is a hard startup error ("Cannot define multiple Listeners"), so
//!   one may only be added where the user's config has none.
//! - **Where module files live.** Laragon ships `mod_proxy_fcgi` but leaves its
//!   `LoadModule` commented out; the commented line still says which directory to load it
//!   from.
//!
//! Only `<IfModule>` is evaluated, and only for `mod_ssl`: Debian's `ports.conf` wraps
//! `Listen 443` in `<IfModule ssl_module>`, which is live only if `ssl.load` was enabled
//! earlier. Every other conditional is treated as true — the scan answers "is there a
//! `Listen` on this port", not "what exactly would httpd do".

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct ScanResult {
    pub listen_ports: Vec<u16>,
    /// Directories holding the `.so` named by a `LoadModule` line — commented-out lines
    /// included, in the order they were found.
    pub module_dirs: Vec<PathBuf>,
}

/// Include nesting deeper than this is either a loop the visited-set missed (a symlink)
/// or not a configuration anyone writes by hand.
const MAX_DEPTH: usize = 16;

/// `skip` is DevsPro's own include, left out so the answer describes the user's config
/// rather than whatever DevsPro added to it last time.
pub fn scan(config_path: &Path, server_root: &Path, skip: &Path) -> ScanResult {
    let mut scanner = Scanner {
        server_root: server_root.to_path_buf(),
        defines: HashMap::new(),
        skip: skip.to_path_buf(),
        visited: Vec::new(),
        ssl_loaded: false,
        conditions: Vec::new(),
        result: ScanResult::default(),
    };
    scanner.scan_file(config_path, 0);
    scanner.result
}

struct Scanner {
    server_root: PathBuf,
    defines: HashMap<String, String>,
    skip: PathBuf,
    visited: Vec<PathBuf>,
    ssl_loaded: bool,
    /// One entry per open `<IfModule>`: whether its body is live.
    conditions: Vec<bool>,
    result: ScanResult,
}

impl Scanner {
    fn live(&self) -> bool {
        self.conditions.iter().all(|c| *c)
    }

    fn scan_file(&mut self, path: &Path, depth: usize) {
        if depth > MAX_DEPTH || same_path(path, &self.skip) || self.visited.iter().any(|v| same_path(v, path)) {
            return;
        }
        self.visited.push(path.to_path_buf());
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        for line in logical_lines(&text) {
            self.scan_line(&line, depth);
        }
    }

    fn scan_line(&mut self, line: &str, depth: usize) {
        let trimmed = line.trim();
        if let Some(comment) = trimmed.strip_prefix('#') {
            let words = words(comment.trim());
            if words.len() >= 3 && words[0].eq_ignore_ascii_case("LoadModule") {
                self.record_module_dir(&words[2]);
            }
            return;
        }
        let words = words(trimmed);
        let Some(directive) = words.first() else {
            return;
        };
        let directive = directive.to_ascii_lowercase();
        let args = &words[1..];

        match directive.as_str() {
            "define" if args.len() >= 2 => {
                if let Some(value) = self.expand(&args[1]) {
                    self.defines.insert(args[0].clone(), value);
                }
            }
            "serverroot" if !args.is_empty() => {
                if let Some(root) = self.expand(&args[0]) {
                    self.server_root = PathBuf::from(root);
                }
            }
            "loadmodule" if args.len() >= 2 => {
                if self.live() && args[0] == "ssl_module" {
                    self.ssl_loaded = true;
                }
                self.record_module_dir(&args[1]);
            }
            "<ifmodule" => {
                let name = args.first().map(|a| a.trim_end_matches('>')).unwrap_or_default();
                let (negated, name) = match name.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, name),
                };
                let holds = if name == "ssl_module" || name == "mod_ssl.c" {
                    self.ssl_loaded
                } else {
                    true
                };
                self.conditions.push(holds != negated);
            }
            "</ifmodule>" => {
                self.conditions.pop();
            }
            "listen" if !args.is_empty() && self.live() => {
                if let Some(port) = listen_port(&args[0]) {
                    if !self.result.listen_ports.contains(&port) {
                        self.result.listen_ports.push(port);
                    }
                }
            }
            "include" | "includeoptional" if !args.is_empty() && self.live() => {
                let Some(pattern) = self.expand(&args[0]) else {
                    return;
                };
                for file in self.resolve_include(&pattern) {
                    self.scan_file(&file, depth + 1);
                }
            }
            _ => {}
        }
    }

    /// `${NAME}` from an earlier `Define`. `None` when one is unknown — Debian's
    /// `${APACHE_LOG_DIR}` comes from the environment `apache2ctl` sets up, and a guessed
    /// path is worse than not following that include.
    fn expand(&self, value: &str) -> Option<String> {
        let mut out = String::new();
        let mut rest = value;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find('}')?;
            out.push_str(self.defines.get(&after[..end])?);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Some(out)
    }

    fn absolute(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.server_root.join(path)
        }
    }

    fn record_module_dir(&mut self, module_path: &str) {
        let Some(expanded) = self.expand(module_path) else {
            return;
        };
        let Some(dir) = self.absolute(&expanded).parent().map(Path::to_path_buf) else {
            return;
        };
        if dir.is_dir() && !self.result.module_dirs.iter().any(|d| same_path(d, &dir)) {
            self.result.module_dirs.push(dir);
        }
    }

    /// A file, a directory (every file in it), or a wildcard in the last component —
    /// sorted, as httpd sorts them. Wildcards earlier in the path are rare enough in
    /// practice that they are not followed.
    fn resolve_include(&self, pattern: &str) -> Vec<PathBuf> {
        let path = self.absolute(pattern);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut files: Vec<PathBuf> = if name.contains(['*', '?']) {
            let Some(dir) = path.parent() else {
                return Vec::new();
            };
            read_dir_files(dir)
                .into_iter()
                .filter(|f| {
                    f.file_name()
                        .is_some_and(|n| wildcard_match(&name, &n.to_string_lossy()))
                })
                .collect()
        } else if path.is_dir() {
            read_dir_files(&path)
        } else {
            vec![path]
        };
        files.sort();
        files
    }
}

fn read_dir_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default()
}

fn same_path(a: &Path, b: &Path) -> bool {
    let normal = |p: &Path| p.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    if cfg!(windows) {
        normal(a) == normal(b)
    } else {
        a == b
    }
}

/// Joins `\`-continued lines, as httpd does before reading a directive.
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for raw in text.lines() {
        match raw.strip_suffix('\\') {
            Some(head) => {
                current.push_str(head);
                current.push(' ');
            }
            None => {
                current.push_str(raw);
                lines.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Whitespace-separated words, with `"..."` kept as one word and its quotes removed.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        if c == '"' {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' if matches!(chars.peek(), Some('"') | Some('\\')) => word.push(chars.next().unwrap()),
                    '"' => break,
                    _ => word.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
        }
        out.push(word);
    }
    out
}

/// `80`, `0.0.0.0:80`, `[::]:443` — the port is whatever follows the last colon.
fn listen_port(arg: &str) -> Option<u16> {
    arg.rsplit(':').next()?.parse().ok()
}

fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a == b && go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo has no `tempfile` dev-dependency; `workers` and `ca` use this same
    /// temp_dir + uuid pattern.
    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-apache-scan-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn slash(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn follows_absolute_includes_and_defines_like_laragon() {
        let root = temp_root();
        let etc = root.join("etc");
        write(&root.join("modules").join("mod_ssl.so"), "");
        write(&etc.join("httpd-ssl.conf"), "Listen 443\nSSLProtocol all\n");
        write(
            &root.join("conf").join("httpd.conf"),
            &format!(
                "Define SRVROOT \"{}\"\nServerRoot \"${{SRVROOT}}\"\nListen 80\n#LoadModule proxy_module modules/mod_proxy.so\nLoadModule ssl_module modules/mod_ssl.so\nInclude \"{}/httpd-ssl.conf\"\n",
                slash(&root),
                slash(&etc)
            ),
        );

        let result = scan(&root.join("conf").join("httpd.conf"), Path::new("/nowhere"), Path::new("/skip"));
        assert_eq!(result.listen_ports, vec![80, 443]);
        assert_eq!(result.module_dirs.len(), 1, "commented and live lines name the same dir");
        assert!(same_path(&result.module_dirs[0], &root.join("modules")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_listen_guarded_by_ssl_counts_only_once_ssl_is_loaded() {
        let root = temp_root();
        let ports = "Listen 80\n<IfModule ssl_module>\n    Listen 443\n</IfModule>\n";

        // Debian with mod_ssl disabled: ports.conf's Listen 443 is dead.
        write(&root.join("plain").join("apache2.conf"), "Include ports.conf\n");
        write(&root.join("plain").join("ports.conf"), ports);
        let result = scan(&root.join("plain").join("apache2.conf"), &root.join("plain"), Path::new("/skip"));
        assert_eq!(result.listen_ports, vec![80]);

        // ... and with `a2enmod ssl`, it is live.
        write(
            &root.join("ssl").join("apache2.conf"),
            "IncludeOptional mods-enabled/*.load\nInclude ports.conf\n",
        );
        write(&root.join("ssl").join("ports.conf"), ports);
        write(
            &root.join("ssl").join("mods-enabled").join("ssl.load"),
            "LoadModule ssl_module /usr/lib/apache2/modules/mod_ssl.so\n",
        );
        let result = scan(&root.join("ssl").join("apache2.conf"), &root.join("ssl"), Path::new("/skip"));
        assert_eq!(result.listen_ports, vec![80, 443]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn devspro_own_include_is_not_counted() {
        let root = temp_root();
        let own = root.join("devspro.conf");
        write(&own, "Listen 443\n");
        write(&root.join("httpd.conf"), &format!("Listen 80\nInclude \"{}\"\n", slash(&own)));
        let result = scan(&root.join("httpd.conf"), &root, &own);
        assert_eq!(result.listen_ports, vec![80]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_unknown_variable_is_not_guessed_at() {
        let root = temp_root();
        write(&root.join("httpd.conf"), "Include ${APACHE_CONFDIR}/ports.conf\nListen 8080\n");
        let result = scan(&root.join("httpd.conf"), &root, Path::new("/skip"));
        assert_eq!(result.listen_ports, vec![8080]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_listen_addresses_in_every_shape() {
        assert_eq!(listen_port("80"), Some(80));
        assert_eq!(listen_port("0.0.0.0:443"), Some(443));
        assert_eq!(listen_port("[::]:443"), Some(443));
        assert_eq!(listen_port("nonsense"), None);
    }

    #[test]
    fn words_keep_quoted_paths_with_spaces_whole() {
        assert_eq!(
            words(r#"Include "C:/Users/MD. Talha/x.conf""#),
            vec!["Include".to_string(), "C:/Users/MD. Talha/x.conf".to_string()]
        );
    }

    #[test]
    fn wildcards_match_like_a_shell() {
        assert!(wildcard_match("*.conf", "site.conf"));
        assert!(!wildcard_match("*.conf", "site.conf.bak"));
        assert!(wildcard_match("auto.?.conf", "auto.a.conf"));
    }
}
