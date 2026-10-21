//! SSH key management over the user's own `~/.ssh`.
//!
//! Generation shells out to `ssh-keygen` rather than producing keys in Rust:
//! the resulting files are then byte-for-byte what every other tool expects
//! (OpenSSH private key format, passphrase handling, comment), and the
//! fingerprint shown in the UI comes from the same `ssh-keygen -lf` the user
//! would run by hand, so the two can never disagree.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::State;

use super::session::{connect, Credentials};

#[derive(Debug, Clone, Serialize)]
pub struct SshKey {
    /// File stem: `id_ed25519` for `~/.ssh/id_ed25519.pub`.
    pub name: String,
    /// `ED25519`, `RSA`, `ECDSA`… as `ssh-keygen -lf` reports it.
    #[serde(rename = "type")]
    pub key_type: String,
    pub bits: u32,
    pub fingerprint: String,
    pub comment: String,
    /// The full `.pub` line, ready to paste into an authorized_keys file.
    pub public_key: String,
    /// Whether the matching private file sits beside it. A lone `.pub` can be
    /// listed and installed but not used to connect from this machine.
    pub has_private: bool,
    /// Path of the private key (whether or not it exists) — what a host's
    /// `key_path` should point at.
    pub path: String,
}

/// The parts of one `ssh-keygen -lf` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    pub bits: u32,
    pub fingerprint: String,
    pub comment: String,
    pub key_type: String,
}

/// Parses `256 SHA256:abc... user@host (ED25519)`. The comment is whatever
/// sits between the hash and the trailing `(TYPE)`, spaces included; when a
/// key has none, ssh-keygen prints `no comment`, which is normalised away.
pub fn parse_keygen_fingerprint(line: &str) -> Option<Fingerprint> {
    let line = line.trim();
    let (bits_str, rest) = line.split_once(' ')?;
    let bits = bits_str.parse::<u32>().ok()?;
    let (fingerprint, rest) = rest.trim_start().split_once(' ')?;
    let rest = rest.trim();
    let open = rest.rfind('(')?;
    let key_type = rest[open + 1..].trim_end_matches(')').trim().to_string();
    if key_type.is_empty() {
        return None;
    }
    let comment = rest[..open].trim();
    let comment = if comment == "no comment" { "" } else { comment };
    Some(Fingerprint {
        bits,
        fingerprint: fingerprint.to_string(),
        comment: comment.to_string(),
        key_type,
    })
}

/// A key name becomes a file name under `~/.ssh`, so it must not be able to
/// escape that directory or collide with the dotfiles that live there
/// (`config`, `known_hosts`, `authorized_keys`, and anything hidden).
pub fn validate_key_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("key name cannot be empty".to_string());
    }
    if name.starts_with('.') {
        return Err("key name cannot start with a dot".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("key name cannot contain slashes".to_string());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err("key name may only contain letters, digits, dots, underscores and dashes".to_string());
    }
    if name.ends_with(".pub") {
        return Err("name the private key; the .pub suffix is added automatically".to_string());
    }
    const RESERVED: [&str; 4] = ["config", "known_hosts", "authorized_keys", "environment"];
    if RESERVED.contains(&name) {
        return Err(format!("\"{name}\" is a reserved file name in ~/.ssh"));
    }
    Ok(name.to_string())
}

/// The shell snippet that does what `ssh-copy-id` does, in one round trip:
/// create `~/.ssh` (700) and `authorized_keys` (600) if missing, append the
/// key only if that exact line isn't there already, and print which happened.
/// The key is single-quoted; a `'` in the comment is the only character that
/// could break out, and it is escaped the POSIX way.
pub fn install_script(public_key: &str) -> String {
    let quoted = public_key.trim().replace('\'', "'\\''");
    format!(
        "umask 077; mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && \
         if grep -qxF '{quoted}' ~/.ssh/authorized_keys; then echo present; else printf '%s\\n' '{quoted}' >> ~/.ssh/authorized_keys && echo added; fi"
    )
}

fn ssh_keygen_fingerprint(pub_path: &Path) -> Option<Fingerprint> {
    let output = super::openssh_command("ssh-keygen").arg("-lf").arg(pub_path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_keygen_fingerprint(&String::from_utf8_lossy(&output.stdout))
}

/// Fallback when `ssh-keygen` is unavailable: the `.pub` line itself carries
/// the algorithm and comment, just not the hash or size.
fn parse_pub_line(line: &str) -> (String, String) {
    let mut parts = line.split_whitespace();
    let algo = parts.next().unwrap_or("").to_string();
    let _key = parts.next();
    let comment = parts.collect::<Vec<_>>().join(" ");
    let key_type = algo
        .strip_prefix("ssh-")
        .unwrap_or(&algo)
        .split('-')
        .next()
        .unwrap_or("")
        .to_uppercase();
    (key_type, comment)
}

fn read_key(pub_path: &Path) -> Result<SshKey, String> {
    let public_key = std::fs::read_to_string(pub_path)
        .map_err(|e| format!("failed to read {}: {e}", pub_path.display()))?
        .trim()
        .to_string();
    let name = pub_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid key file name")?
        .to_string();
    let private_path = pub_path.with_extension("");
    let (fallback_type, fallback_comment) = parse_pub_line(&public_key);
    let fp = ssh_keygen_fingerprint(pub_path);
    Ok(SshKey {
        name,
        key_type: fp.as_ref().map(|f| f.key_type.clone()).unwrap_or(fallback_type),
        bits: fp.as_ref().map(|f| f.bits).unwrap_or(0),
        fingerprint: fp.as_ref().map(|f| f.fingerprint.clone()).unwrap_or_default(),
        comment: fp.map(|f| f.comment).unwrap_or(fallback_comment),
        public_key,
        has_private: private_path.is_file(),
        path: private_path.to_string_lossy().into_owned(),
    })
}

pub fn list_in(dir: &Path) -> Result<Vec<SshKey>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pub") || !path.is_file() {
            continue;
        }
        match read_key(&path) {
            Ok(key) => keys.push(key),
            // A malformed .pub shouldn't hide every other key.
            Err(_) => continue,
        }
    }
    keys.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(keys)
}

#[tauri::command(async)]
pub fn ssh_keys_list() -> Result<Vec<SshKey>, String> {
    list_in(&super::ssh_dir()?)
}

#[derive(Debug, Deserialize)]
pub struct GenerateKeyArgs {
    pub name: String,
    pub key_type: String,
    pub bits: Option<u32>,
    pub comment: String,
    pub passphrase: String,
}

/// Arguments for `ssh-keygen`, kept separate so they can be checked without
/// spawning anything. `-q` because the ASCII randomart would otherwise be the
/// only output and it is useless in a log. `-N ""` is what makes an empty
/// passphrase non-interactive.
pub fn keygen_args(args: &GenerateKeyArgs, private_path: &Path) -> Result<Vec<String>, String> {
    let mut out = vec!["-q".to_string(), "-t".to_string()];
    match args.key_type.as_str() {
        "ed25519" => out.push("ed25519".to_string()),
        "rsa" => {
            let bits = args.bits.unwrap_or(4096);
            if !(2048..=16384).contains(&bits) {
                return Err("RSA key size must be between 2048 and 16384 bits".to_string());
            }
            out.push("rsa".to_string());
            out.push("-b".to_string());
            out.push(bits.to_string());
        }
        other => return Err(format!("unsupported key type \"{other}\" (expected ed25519 or rsa)")),
    }
    out.push("-f".to_string());
    out.push(private_path.to_string_lossy().into_owned());
    out.push("-N".to_string());
    out.push(args.passphrase.clone());
    out.push("-C".to_string());
    out.push(args.comment.trim().to_string());
    Ok(out)
}

#[tauri::command(async)]
pub fn ssh_keys_generate(args: GenerateKeyArgs) -> Result<SshKey, String> {
    let name = validate_key_name(&args.name)?;
    let dir = super::ensure_ssh_dir()?;
    let private_path = dir.join(&name);
    let pub_path = dir.join(format!("{name}.pub"));
    if private_path.exists() || pub_path.exists() {
        return Err(format!("a key named \"{name}\" already exists in ~/.ssh"));
    }
    let argv = keygen_args(&args, &private_path)?;
    let output = super::openssh_command("ssh-keygen")
        .args(&argv)
        .output()
        .map_err(|e| format!("failed to run ssh-keygen: {e}{}", super::openssh_missing_hint()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ssh-keygen failed: {}", stderr.trim()));
    }
    read_key(&pub_path)
}

#[derive(Debug, Deserialize)]
pub struct ImportKeyArgs {
    pub name: String,
    pub private_key: String,
    /// Optional: worked out from the private key when empty.
    pub public_key: String,
    /// Only for working out the public half of a key that has one; never stored.
    pub passphrase: String,
}

/// Pasted text in the shape OpenSSH reads: LF line endings (a key copied on
/// Windows arrives with CRLF, which ssh rejects as "invalid format"), no blank
/// lines around it, and a trailing newline, without which ssh also refuses it.
pub fn normalize_private_key(text: &str) -> Result<String, String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    if text.starts_with("PuTTY-User-Key-File") {
        return Err(
            "this is a PuTTY .ppk key; convert it first with `puttygen key.ppk -O private-openssh -o key`"
                .to_string(),
        );
    }
    if !(text.starts_with("-----BEGIN ") && text.contains("PRIVATE KEY-----") && text.ends_with("-----")) {
        return Err("that doesn't look like a private key; it should start with -----BEGIN … PRIVATE KEY-----".to_string());
    }
    Ok(format!("{text}\n"))
}

/// One `.pub` line, or `None` when nothing was pasted. A line wrapped by the
/// clipboard is joined back up: the key itself never contains whitespace.
pub fn normalize_public_key(text: &str) -> Result<Option<String>, String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return Ok(None);
    }
    let algo = line.split(' ').next().unwrap_or("");
    let known = ["ssh-", "ecdsa-", "sk-"].iter().any(|p| algo.starts_with(p));
    if !known || line.split(' ').count() < 2 {
        return Err("that doesn't look like a public key; it should start with ssh-ed25519, ssh-rsa or ecdsa-…".to_string());
    }
    Ok(Some(line))
}

/// The algorithm and key blob, which are the key; the comment isn't.
fn key_identity(line: &str) -> (String, String) {
    let mut parts = line.split_whitespace();
    (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string())
}

/// The private file, created owner-only from the first byte: chmod after the
/// write would leave a moment where anyone could read it, and ssh refuses a
/// key that others can read anyway.
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    // Windows has no creation mode. The new file inherits its folder's ACL —
    // under the user's profile already you, SYSTEM and Administrators — and is
    // then cut down to you alone, as ssh-keygen does for a key it generates,
    // before anything (including the `ssh-keygen -y` below) reads it.
    #[cfg(windows)]
    if let Err(e) = super::key_perms::restrict(path) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// What the private key says its public half is, via `ssh-keygen -y`. Errs
/// with ssh-keygen's own words, which name a wrong or missing passphrase.
fn derive_public(private_path: &Path, passphrase: &str) -> Result<String, String> {
    let output = super::openssh_command("ssh-keygen")
        .arg("-y")
        .arg("-P")
        .arg(passphrase)
        .arg("-f")
        .arg(private_path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("failed to run ssh-keygen: {e}{}", super::openssh_missing_hint()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Saves a pasted key pair as `<dir>/<name>` and `<name>.pub`, the same two
/// files Generate makes, so it shows up wherever a key can be picked. Nothing
/// is left behind when the key is refused.
pub fn import_into(dir: &Path, args: &ImportKeyArgs) -> Result<SshKey, String> {
    let name = validate_key_name(&args.name)?;
    let private = normalize_private_key(&args.private_key)?;
    let public = normalize_public_key(&args.public_key)?;
    let private_path = dir.join(&name);
    let pub_path = dir.join(format!("{name}.pub"));
    if private_path.exists() || pub_path.exists() {
        return Err(format!("a key named \"{name}\" already exists in ~/.ssh"));
    }

    write_private(&private_path, &private)?;
    let public_line = match (derive_public(&private_path, &args.passphrase), public) {
        (Ok(derived), Some(pasted)) if key_identity(&derived) != key_identity(&pasted) => {
            Err("the public key doesn't belong to this private key".to_string())
        }
        // The pasted line keeps its comment; the derived one may have none.
        (Ok(_), Some(pasted)) => Ok(pasted),
        (Ok(derived), None) if derived.split_whitespace().count() < 3 => Ok(format!("{derived} {name}")),
        (Ok(derived), None) => Ok(derived),
        // A key with a passphrase can't be checked without it, but the pasted
        // public half is all the .pub file needs.
        (Err(e), Some(pasted)) if e.contains("passphrase") && args.passphrase.is_empty() => Ok(pasted),
        (Err(e), None) if e.contains("passphrase") && args.passphrase.is_empty() => Err(
            "this key has a passphrase: enter it (it's only used to work out the public key, not saved), or paste the public key too"
                .to_string(),
        ),
        (Err(e), _) if e.contains("passphrase") => Err("wrong passphrase for this key".to_string()),
        (Err(e), _) => Err(format!("ssh-keygen couldn't read this key: {e}")),
    };
    let saved = public_line.and_then(|line| {
        std::fs::write(&pub_path, format!("{line}\n"))
            .map_err(|e| format!("failed to write {}: {e}", pub_path.display()))
    });
    if let Err(e) = saved {
        let _ = std::fs::remove_file(&private_path);
        let _ = std::fs::remove_file(&pub_path);
        return Err(e);
    }
    read_key(&pub_path)
}

#[tauri::command(async)]
pub fn ssh_keys_import(args: ImportKeyArgs) -> Result<SshKey, String> {
    // Name first, so a bad one is reported before ~/.ssh gets created for it.
    validate_key_name(&args.name)?;
    import_into(&super::ensure_ssh_dir()?, &args)
}

#[tauri::command(async)]
pub fn ssh_keys_delete(name: String) -> Result<(), String> {
    let name = validate_key_name(&name)?;
    let dir = super::ssh_dir()?;
    let private_path = dir.join(&name);
    let pub_path = dir.join(format!("{name}.pub"));
    if !private_path.exists() && !pub_path.exists() {
        return Err(format!("no key named \"{name}\" in ~/.ssh"));
    }
    for path in [private_path, pub_path] {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// `ssh-copy-id`, minus the shell: appends the public key to the remote
/// account's authorized_keys over an authenticated session. The password (or
/// key passphrase) is for *this* connection only — the whole point is that
/// the next one won't need it.
#[tauri::command(async)]
pub fn ssh_keys_install_on_host(
    db: State<crate::db::Db>,
    host_id: String,
    key_name: String,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<String, String> {
    let name = validate_key_name(&key_name)?;
    let pub_path = super::ssh_dir()?.join(format!("{name}.pub"));
    let public_key = std::fs::read_to_string(&pub_path)
        .map_err(|e| format!("failed to read {}: {e}", pub_path.display()))?;
    let public_key = public_key.trim();
    if public_key.is_empty() {
        return Err("public key file is empty".to_string());
    }

    let host = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::hosts::find(&conn, &host_id)?
    };
    let sess = connect(&host, &Credentials { password, passphrase })?;
    let result = run_remote(&sess, &install_script(public_key))?;
    let _ = sess.disconnect(None, "done", None);
    Ok(match result.trim() {
        "present" => format!("Key \"{name}\" was already authorized on {}.", host.name),
        _ => format!("Key \"{name}\" added to authorized_keys on {}.", host.name),
    })
}

fn run_remote(sess: &ssh2::Session, script: &str) -> Result<String, String> {
    use std::io::Read;
    let mut channel = sess.channel_session().map_err(|e| e.message().to_string())?;
    channel.exec(script).map_err(|e| e.message().to_string())?;
    let mut stdout = String::new();
    channel.read_to_string(&mut stdout).map_err(|e| e.to_string())?;
    let mut stderr = String::new();
    let _ = channel.stderr().read_to_string(&mut stderr);
    let _ = channel.wait_close();
    let status = channel.exit_status().unwrap_or(-1);
    if status != 0 {
        return Err(format!("remote command failed (exit {status}): {}", stderr.trim()));
    }
    Ok(stdout)
}

/// Where a `.pub` lives for a given key name; used by the UI-facing path
/// reported in `SshKey::path`.
#[allow(dead_code)]
pub fn public_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.pub"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_standard_keygen_line() {
        let fp = parse_keygen_fingerprint(
            "256 SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8 me@laptop (ED25519)\n",
        )
        .unwrap();
        assert_eq!(fp.bits, 256);
        assert_eq!(fp.fingerprint, "SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8");
        assert_eq!(fp.comment, "me@laptop");
        assert_eq!(fp.key_type, "ED25519");
    }

    #[test]
    fn parses_comments_with_spaces_and_no_comment() {
        let fp = parse_keygen_fingerprint("3072 SHA256:abc work laptop key (RSA)").unwrap();
        assert_eq!(fp.comment, "work laptop key");
        assert_eq!(fp.key_type, "RSA");
        let fp = parse_keygen_fingerprint("3072 SHA256:abc no comment (RSA)").unwrap();
        assert_eq!(fp.comment, "");
    }

    #[test]
    fn rejects_garbage_lines() {
        assert_eq!(parse_keygen_fingerprint(""), None);
        assert_eq!(parse_keygen_fingerprint("not a fingerprint"), None);
        assert_eq!(parse_keygen_fingerprint("256 SHA256:abc"), None);
    }

    #[test]
    fn key_names_are_confined_to_ssh_dir() {
        assert_eq!(validate_key_name("  id_ed25519 ").unwrap(), "id_ed25519");
        assert!(validate_key_name("work-2024.key").is_ok());
        assert!(validate_key_name("").is_err());
        assert!(validate_key_name(".hidden").is_err());
        assert!(validate_key_name("../etc/passwd").is_err());
        assert!(validate_key_name("a/b").is_err());
        assert!(validate_key_name("a\\b").is_err());
        assert!(validate_key_name("has space").is_err());
        assert!(validate_key_name("id.pub").is_err());
        assert!(validate_key_name("config").is_err());
        assert!(validate_key_name("authorized_keys").is_err());
    }

    #[test]
    fn install_script_quotes_the_key_and_is_idempotent_by_design() {
        let script = install_script("ssh-ed25519 AAAA it's me\n");
        assert!(script.contains("grep -qxF 'ssh-ed25519 AAAA it'\\''s me'"));
        assert!(script.contains("chmod 700 ~/.ssh"));
        assert!(script.contains("chmod 600 ~/.ssh/authorized_keys"));
        assert!(script.contains("echo present"));
        assert!(script.contains("echo added"));
    }

    #[test]
    fn keygen_args_default_rsa_bits_and_reject_unknown_types() {
        let base = |t: &str, bits: Option<u32>| GenerateKeyArgs {
            name: "k".into(),
            key_type: t.into(),
            bits,
            comment: " me@box ".into(),
            passphrase: String::new(),
        };
        let ed = keygen_args(&base("ed25519", None), Path::new("/tmp/k")).unwrap();
        assert_eq!(ed, ["-q", "-t", "ed25519", "-f", "/tmp/k", "-N", "", "-C", "me@box"]);
        let rsa = keygen_args(&base("rsa", None), Path::new("/tmp/k")).unwrap();
        assert!(rsa.windows(2).any(|w| w == ["-b", "4096"]));
        assert!(keygen_args(&base("rsa", Some(512)), Path::new("/tmp/k")).is_err());
        assert!(keygen_args(&base("dsa", None), Path::new("/tmp/k")).is_err());
    }

    #[test]
    fn pub_line_fallback_extracts_type_and_comment() {
        assert_eq!(
            parse_pub_line("ssh-ed25519 AAAAC3 me@host"),
            ("ED25519".to_string(), "me@host".to_string())
        );
        assert_eq!(parse_pub_line("ssh-rsa AAAAB3"), ("RSA".to_string(), String::new()));
    }

    #[test]
    fn pasted_private_keys_are_normalised_or_refused() {
        let pasted = "\r\n-----BEGIN OPENSSH PRIVATE KEY-----\r\nabc\r\n-----END OPENSSH PRIVATE KEY-----  \r\n";
        assert_eq!(
            normalize_private_key(pasted).unwrap(),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n"
        );
        assert!(normalize_private_key("PuTTY-User-Key-File-3: ssh-ed25519\n").unwrap_err().contains("puttygen"));
        assert!(normalize_private_key("ssh-ed25519 AAAA me").is_err());
        assert!(normalize_private_key("").is_err());
    }

    #[test]
    fn pasted_public_keys_are_joined_onto_one_line() {
        assert_eq!(normalize_public_key("  \n").unwrap(), None);
        assert_eq!(
            normalize_public_key("ssh-ed25519 AAAA\nC3 me@host\n").unwrap().unwrap(),
            "ssh-ed25519 AAAA C3 me@host"
        );
        assert!(normalize_public_key("-----BEGIN OPENSSH PRIVATE KEY-----").is_err());
        assert!(normalize_public_key("ssh-ed25519").is_err());
    }

    /// Runs real `ssh-keygen` against a temp dir; skipped where it's missing.
    fn keygen_pair(dir: &Path, name: &str, passphrase: &str) -> Option<(String, String)> {
        let path = dir.join(name);
        let ok = crate::modules::ssh::openssh_command("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", passphrase, "-C", "me@box", "-f"])
            .arg(&path)
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
        let pair = (std::fs::read_to_string(&path).ok()?, std::fs::read_to_string(path.with_extension("pub")).ok()?);
        std::fs::remove_file(&path).ok()?;
        std::fs::remove_file(path.with_extension("pub")).ok()?;
        Some(pair)
    }

    #[test]
    fn imports_a_pasted_pair_and_leaves_nothing_behind_when_refused() {
        let dir = std::env::temp_dir().join(format!("devspro-ssh-import-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let Some((private, public)) = keygen_pair(&dir, "src", "") else { return };
        let Some((other_private, other_public)) = keygen_pair(&dir, "other", "") else { return };
        let Some((locked, locked_public)) = keygen_pair(&dir, "locked", "s3cret") else { return };
        let args = |name: &str, private: &str, public: &str, passphrase: &str| ImportKeyArgs {
            name: name.into(),
            private_key: private.into(),
            public_key: public.into(),
            passphrase: passphrase.into(),
        };

        // Private key only, pasted with Windows line endings: the public half is worked out.
        let key = import_into(&dir, &args("work", &private.replace('\n', "\r\n"), "", "")).unwrap();
        assert!(key.has_private);
        assert_eq!(key_identity(&key.public_key), key_identity(&public));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("work")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(import_into(&dir, &args("work", &private, "", "")).unwrap_err().contains("already exists"));

        // A matching public key keeps its comment; someone else's is refused.
        assert_eq!(import_into(&dir, &args("paired", &private, &public, "")).unwrap().comment, "me@box");
        let mismatch = import_into(&dir, &args("mixed", &private, &other_public, "")).unwrap_err();
        assert!(mismatch.contains("doesn't belong"));
        assert!(!dir.join("mixed").exists() && !dir.join("mixed.pub").exists());
        let _ = other_private;

        // A key with a passphrase: needs it, or its public half.
        let needs = import_into(&dir, &args("locked", &locked, "", "")).unwrap_err();
        assert!(needs.contains("passphrase"));
        assert!(!dir.join("locked").exists());
        assert!(import_into(&dir, &args("locked", &locked, "", "wrong")).unwrap_err().contains("wrong passphrase"));
        assert!(import_into(&dir, &args("locked", &locked, "", "s3cret")).is_ok());
        assert!(import_into(&dir, &args("locked2", &locked, &locked_public, "")).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_in_reads_pub_files_and_detects_private_halves() {
        let dir = std::env::temp_dir().join(format!("devspro-ssh-keys-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("with_private.pub"), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxKZ2V0dGluZyB0aGlzIHRvIHBhcnNlIGlzIGhhcmQ= a@b\n").unwrap();
        std::fs::write(dir.join("with_private"), "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        std::fs::write(dir.join("lonely.pub"), "ssh-rsa AAAAB3 c@d\n").unwrap();
        std::fs::write(dir.join("known_hosts"), "").unwrap();

        let keys = list_in(&dir).unwrap();
        let names: Vec<_> = keys.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["lonely", "with_private"]);
        assert!(!keys[0].has_private);
        assert!(keys[1].has_private);
        assert_eq!(keys[1].path, dir.join("with_private").to_string_lossy());
        assert!(list_in(&dir.join("nope")).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
