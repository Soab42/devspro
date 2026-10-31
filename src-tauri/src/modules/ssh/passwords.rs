//! Remembered passwords for password-auth hosts, opt-in per connection.
//!
//! The prompt offers "Remember this password", off by default; without it the
//! password goes to the one session and is dropped with it, as before. A
//! remembered password is sealed by `crate::secrets` with its own key in
//! `<app data>/ssh/passwords.key` and is written only after it has actually
//! logged in, so a typo is never kept. One the server later rejects is
//! forgotten, and the user is asked again.
//!
//! The terminal runs the system `ssh`, which asks for the password itself, so
//! there a remembered password is typed into the session for it: see
//! `LoginAutofill`, which `pty.rs` feeds the session's output.
//!
//! Not part of the config backup: the key file stays on this machine, so a
//! sealed password would be useless anywhere else.

use super::hosts::SshHost;
use super::session::{connect, Credentials};
use rusqlite::{Connection, OptionalExtension};
use ssh2::Session;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// Appended to `db::later_migrations()`.
pub const MIGRATION_SSH_PASSWORDS: &str = r#"
CREATE TABLE ssh_saved_passwords (
    host_id TEXT PRIMARY KEY,
    secret TEXT NOT NULL,
    saved_at TEXT NOT NULL
);
"#;

/// What a connect fails with when the remembered password was turned down.
/// The frontend asks for the password again, saying why.
pub const SAVED_PASSWORD_REJECTED: &str = "SAVED_PASSWORD_REJECTED";

fn key_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("ssh")
        .join("passwords.key"))
}

#[cfg(test)]
fn has_saved(conn: &Connection, host_id: &str) -> Result<bool, String> {
    conn.query_row("SELECT 1 FROM ssh_saved_passwords WHERE host_id = ?1", [host_id], |_| Ok(()))
        .optional()
        .map(|found| found.is_some())
        .map_err(|e| e.to_string())
}

fn stored(conn: &Connection, host_id: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT secret FROM ssh_saved_passwords WHERE host_id = ?1", [host_id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

fn store(conn: &Connection, host_id: &str, sealed: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO ssh_saved_passwords (host_id, secret, saved_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(host_id) DO UPDATE SET secret = excluded.secret, saved_at = excluded.saved_at",
        (host_id, sealed, chrono::Utc::now().to_rfc3339()),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn forget(conn: &Connection, host_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM ssh_saved_passwords WHERE host_id = ?1", [host_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The remembered password for a host, or None. One that can't be decrypted
/// (key file replaced or lost) is dropped, so the user is simply asked again.
pub fn recall(app: &AppHandle, db: &crate::db::Db, host_id: &str) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let Some(sealed) = stored(&conn, host_id)? else { return Ok(None) };
    match crate::secrets::cipher_at(&key_path(app)?).and_then(|c| crate::secrets::open(&c, &sealed)) {
        Ok(password) => Ok(Some(password)),
        Err(_) => {
            forget(&conn, host_id)?;
            Ok(None)
        }
    }
}

fn remember(app: &AppHandle, db: &crate::db::Db, host_id: &str, password: &str) -> Result<(), String> {
    let sealed = crate::secrets::seal(&crate::secrets::cipher_at(&key_path(app)?)?, password)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    store(&conn, host_id, &sealed)
}

/// `connect`, with remembered passwords: a password host connected without a
/// typed password uses the remembered one; a typed password is remembered
/// after it logs in when `remember` is set.
pub fn connect_host(
    app: &AppHandle,
    db: &crate::db::Db,
    host: &SshHost,
    password: Option<String>,
    passphrase: Option<String>,
    remember_it: bool,
) -> Result<Session, String> {
    let typed = password.is_some();
    let password = match password {
        Some(p) => Some(p),
        None if host.auth == "password" => recall(app, db, &host.id)?,
        None => None,
    };
    let from_saved = !typed && password.is_some();
    let creds = Credentials { password, passphrase };
    match connect(host, &creds) {
        Ok(session) => {
            if typed && remember_it && host.auth == "password" {
                if let Some(pw) = creds.password.as_deref() {
                    // Connected either way; failing to store just means asking next time.
                    if let Err(e) = remember(app, db, &host.id, pw) {
                        eprintln!("ssh: could not remember the password for {}: {e}", host.name);
                    }
                }
            }
            Ok(session)
        }
        Err(e) if from_saved && e.contains("password rejected") => {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            forget(&conn, &host.id)?;
            Err(SAVED_PASSWORD_REJECTED.to_string())
        }
        Err(e) => Err(e),
    }
}

/// How much of the start of a session is watched for the login prompt. A
/// prompt this far in is not ssh's login any more, so nothing is typed.
const AUTOFILL_WINDOW: usize = 64 * 1024;

/// What `LoginAutofill::feed` asks the session to do.
#[derive(Debug, PartialEq)]
pub enum Autofill {
    Nothing,
    /// Write this to the terminal: the password and Enter.
    Type(String),
    /// The server turned the typed password down: forget it.
    Rejected,
}

/// Types a remembered password at ssh's login prompt in a terminal session.
///
/// Only ssh's own prompt for this host — `user@host's password:` — counts,
/// so a remote `sudo` or another program asking for a password later never
/// gets it. It types at most once: a rejected password is not retried (ssh
/// asks again, and the user answers), and is reported so it can be forgotten.
pub struct LoginAutofill {
    prompt: String,
    password: Option<String>,
    tail: String,
    seen: usize,
    typed: bool,
    done: bool,
}

impl LoginAutofill {
    pub fn new(host: &SshHost, password: String) -> Self {
        LoginAutofill {
            prompt: format!("{}@{}'s password:", host.username, host.host),
            password: Some(password),
            tail: String::new(),
            seen: 0,
            typed: false,
            done: false,
        }
    }

    /// Looks at the next chunk of the session's output.
    pub fn feed(&mut self, chunk: &[u8]) -> Autofill {
        if self.done {
            return Autofill::Nothing;
        }
        self.seen += chunk.len();
        self.tail.push_str(&String::from_utf8_lossy(chunk));
        // Enough to hold the prompt across chunk boundaries, and no more.
        if self.tail.len() > 1024 {
            let cut = self.tail.len() - 512;
            let cut = (cut..self.tail.len()).find(|&i| self.tail.is_char_boundary(i)).unwrap_or(0);
            self.tail.drain(..cut);
        }

        if self.typed {
            if self.tail.contains("Permission denied") || self.tail.trim_end().ends_with(&self.prompt) {
                self.done = true;
                return Autofill::Rejected;
            }
            // Anything else after a newline means the login went through.
            if self.tail.contains('\n') {
                self.done = true;
            }
            return Autofill::Nothing;
        }

        if self.seen > AUTOFILL_WINDOW {
            self.done = true;
            self.password = None;
            return Autofill::Nothing;
        }
        if self.tail.trim_end().ends_with(&self.prompt) {
            self.typed = true;
            self.tail.clear();
            if let Some(pw) = self.password.take() {
                return Autofill::Type(format!("{pw}\r"));
            }
        }
        Autofill::Nothing
    }
}

#[tauri::command(async)]
pub fn ssh_password_forget(db: State<crate::db::Db>, host_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    forget(&conn, &host_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_SSH_PASSWORDS).unwrap();
        conn
    }

    #[test]
    fn store_replaces_and_forget_removes() {
        let conn = db();
        assert!(!has_saved(&conn, "h1").unwrap());
        store(&conn, "h1", "sealed-one").unwrap();
        store(&conn, "h1", "sealed-two").unwrap();
        assert!(has_saved(&conn, "h1").unwrap());
        assert_eq!(stored(&conn, "h1").unwrap().as_deref(), Some("sealed-two"));
        forget(&conn, "h1").unwrap();
        assert!(!has_saved(&conn, "h1").unwrap());
        assert_eq!(stored(&conn, "h1").unwrap(), None);
    }

    fn web() -> SshHost {
        SshHost {
            id: "h1".into(),
            name: "web".into(),
            host: "10.0.0.5".into(),
            port: 22,
            username: "root".into(),
            auth: "password".into(),
            key_path: None,
            created_at: String::new(),
            has_saved_password: true,
        }
    }

    #[test]
    fn autofill_types_once_at_the_login_prompt_even_split_across_chunks() {
        let mut fill = LoginAutofill::new(&web(), "hunter2".into());
        assert_eq!(fill.feed(b"Warning: Permanently added '10.0.0.5'\r\n"), Autofill::Nothing);
        assert_eq!(fill.feed(b"root@10.0.0"), Autofill::Nothing);
        assert_eq!(fill.feed(b".5's password: "), Autofill::Type("hunter2\r".into()));
        assert_eq!(fill.feed(b"\r\nWelcome to Ubuntu\r\n"), Autofill::Nothing);
        // Later prompts are not ssh's login, and get nothing.
        assert_eq!(fill.feed(b"root@10.0.0.5's password: "), Autofill::Nothing);
        assert_eq!(fill.feed(b"[sudo] password for root: "), Autofill::Nothing);
    }

    #[test]
    fn autofill_reports_a_rejected_password_and_does_not_retry() {
        let mut fill = LoginAutofill::new(&web(), "old".into());
        assert_eq!(fill.feed(b"root@10.0.0.5's password: "), Autofill::Type("old\r".into()));
        assert_eq!(fill.feed(b"\r\nPermission denied, please try again.\r\n"), Autofill::Rejected);
        assert_eq!(fill.feed(b"root@10.0.0.5's password: "), Autofill::Nothing);
    }

    #[test]
    fn autofill_ignores_other_hosts_and_prompts() {
        let mut fill = LoginAutofill::new(&web(), "hunter2".into());
        assert_eq!(fill.feed(b"[sudo] password for root: "), Autofill::Nothing);
        assert_eq!(fill.feed(b"\r\nadmin@10.0.0.5's password: "), Autofill::Nothing);
    }

    #[test]
    fn autofill_gives_up_once_the_session_is_well_under_way() {
        let mut fill = LoginAutofill::new(&web(), "hunter2".into());
        assert_eq!(fill.feed(&vec![b'x'; AUTOFILL_WINDOW + 1]), Autofill::Nothing);
        assert_eq!(fill.feed(b"\r\nroot@10.0.0.5's password: "), Autofill::Nothing);
    }

    #[test]
    fn the_stored_value_is_not_the_password() {
        let dir = std::env::temp_dir().join(format!("devspro-ssh-{}", uuid::Uuid::new_v4()));
        let cipher = crate::secrets::cipher_at(&dir.join("passwords.key")).unwrap();
        let sealed = crate::secrets::seal(&cipher, "hunter2").unwrap();
        let conn = db();
        store(&conn, "h1", &sealed).unwrap();
        let row = stored(&conn, "h1").unwrap().unwrap();
        assert!(!row.contains("hunter2"));
        assert_eq!(crate::secrets::open(&cipher, &row).unwrap(), "hunter2");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
