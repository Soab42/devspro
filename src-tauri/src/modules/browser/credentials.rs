//! Saved logins for the Browser module.
//!
//! Capture: the initialization script in `mod.rs` posts `{type, username,
//! password}` over wry IPC whenever a form with a password field is submitted.
//! The origin is read from the IPC request's URI, which wry fills in from the
//! sending frame; the page cannot forge it. If that exact pair is already
//! saved nothing happens; otherwise the frontend is asked whether to save (or
//! update) it, and only then does anything reach the database.
//!
//! At rest: AES-256-GCM with a random 32-byte key in
//! `<app data>/browser/credentials.key`, created on first use with mode 0600.
//! The same threat model as Chrome on Linux without a keyring — a casual
//! `sqlite3 app.db` doesn't show passwords; someone with the user's files can
//! still read them. The app's profile password isn't used as the key because
//! autofill must work while the vault is unlocked but the profile password is
//! long gone from memory.
//!
//! Autofill: when a page finishes loading and its origin has exactly one
//! saved login, the fields are filled in place. More than one means a choice,
//! which the frontend offers from the key icon in the address bar and applies
//! with `browser_credential_fill`.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

pub const CREDENTIAL_EVENT: &str = "browser:credential";

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Serialize)]
pub struct Credential {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct CredentialEvent {
    id: String,
    origin: String,
    username: String,
    password: String,
    /// True when a login for this origin+username exists with a different
    /// password, so the prompt should say "Update" rather than "Save".
    update: bool,
}

#[derive(Debug, Deserialize)]
struct CapturedMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

fn key_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("browser")
        .join("credentials.key"))
}

fn load_or_create_key(path: &Path) -> Result<[u8; 32], String> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        return Err("the browser credentials key file is damaged".into());
    }
    let key: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, key).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

fn cipher(app: &AppHandle) -> Result<Aes256Gcm, String> {
    let key = load_or_create_key(&key_path(app)?)?;
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)))
}

/// base64(nonce || ciphertext).
fn seal(cipher: &Aes256Gcm, plaintext: &str) -> Result<String, String> {
    let nonce = Aes256Gcm::generate_nonce(OsRng);
    let ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| "could not encrypt the password".to_string())?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(B64.encode(out))
}

fn open(cipher: &Aes256Gcm, sealed: &str) -> Result<String, String> {
    let bytes = B64.decode(sealed).map_err(|_| "stored password is not valid base64".to_string())?;
    if bytes.len() < 12 {
        return Err("stored password is truncated".into());
    }
    let (nonce, ct) = bytes.split_at(12);
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "could not decrypt the password (was the key file replaced?)".to_string())?;
    String::from_utf8(pt).map_err(|_| "stored password is not text".to_string())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn stored_secret(conn: &Connection, origin: &str, username: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT secret FROM browser_credentials WHERE origin = ?1 AND username = ?2",
        params![origin, username],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn upsert(conn: &Connection, origin: &str, username: &str, secret: &str) -> Result<(), String> {
    let stamp = now();
    conn.execute(
        "INSERT INTO browser_credentials (id, origin, username, secret, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(origin, username) DO UPDATE SET secret = excluded.secret, updated_at = excluded.updated_at",
        params![uuid::Uuid::new_v4().to_string(), origin, username, secret, stamp],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Called from the IPC handler on the main thread. Never blocks on the
/// frontend; it only decides whether to ask.
pub(super) fn on_ipc(app: &AppHandle, window_label: &str, tab_id: &str, page_url: &str, body: &str) {
    let Ok(msg) = serde_json::from_str::<CapturedMessage>(body) else { return };
    if msg.kind != "credential" || msg.password.is_empty() {
        return;
    }
    let Some(origin) = super::origin_of(page_url) else { return };

    let update = {
        let Some(db) = app.try_state::<crate::db::Db>() else { return };
        let Ok(conn) = db.0.lock() else { return };
        match stored_secret(&conn, &origin, &msg.username) {
            Ok(Some(sealed)) => match cipher(app).and_then(|c| open(&c, &sealed)) {
                Ok(existing) if existing == msg.password => return, // already saved
                _ => true,
            },
            Ok(None) => false,
            Err(_) => return,
        }
    };

    let _ = app.emit_to(
        window_label,
        CREDENTIAL_EVENT,
        CredentialEvent {
            id: tab_id.to_string(),
            origin,
            username: msg.username,
            password: msg.password,
            update,
        },
    );
}

/// Sets the username and password fields the way a user typing would, so
/// frameworks that track input through React's value setter notice.
fn fill_script(username: &str, password: &str) -> String {
    format!(
        r#"(function () {{
  var u = {u}, p = {p};
  function set(el, v) {{
    if (!el) return;
    var proto = Object.getPrototypeOf(el);
    var d = Object.getOwnPropertyDescriptor(proto, "value");
    if (d && d.set) d.set.call(el, v); else el.value = v;
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
  }}
  var pw = document.querySelector("input[type=password]");
  if (!pw) return;
  var root = pw.form || document;
  var inputs = Array.prototype.slice.call(root.querySelectorAll("input"));
  var user = null;
  for (var i = inputs.indexOf(pw) - 1; i >= 0; i--) {{
    var t = (inputs[i].getAttribute("type") || "text").toLowerCase();
    if (t === "text" || t === "email" || t === "tel") {{ user = inputs[i]; break; }}
  }}
  if (user && u) set(user, u);
  set(pw, p);
}})();"#,
        u = serde_json::to_string(username).unwrap_or_default(),
        p = serde_json::to_string(password).unwrap_or_default(),
    )
}

/// On page load: fill when the origin has exactly one saved login.
/// Called from wry's page-load callback on the GTK main thread. The lookup
/// (database lock, key file, AES) runs on a worker so a page finishing its
/// load never waits on the app database; only the final script injection
/// hops back, because the tab map lives on the main thread.
pub(super) fn autofill(app: &AppHandle, tab_id: &str, url: &str) {
    let Some(origin) = super::origin_of(url) else { return };
    let app = app.clone();
    let tab_id = tab_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(db) = app.try_state::<crate::db::Db>() else { return };
        let row: Option<(String, String)> = {
            let Ok(conn) = db.0.lock() else { return };
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM browser_credentials WHERE origin = ?1", [&origin], |r| r.get(0))
                .unwrap_or(0);
            if count != 1 {
                return;
            }
            conn.query_row(
                "SELECT username, secret FROM browser_credentials WHERE origin = ?1",
                [&origin],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap_or(None)
        };
        let Some((username, sealed)) = row else { return };
        let Ok(password) = cipher(&app).and_then(|c| open(&c, &sealed)) else { return };
        let script = fill_script(&username, &password);
        let _ = app.run_on_main_thread(move || super::eval_in_tab(&tab_id, &script));
    });
}

#[tauri::command(async)]
pub fn browser_credentials_list(db: State<crate::db::Db>) -> Result<Vec<Credential>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, origin, username, updated_at FROM browser_credentials ORDER BY origin, username")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Credential { id: r.get(0)?, origin: r.get(1)?, username: r.get(2)?, updated_at: r.get(3)? })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn browser_credentials_for(db: State<crate::db::Db>, origin: String) -> Result<Vec<Credential>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, origin, username, updated_at FROM browser_credentials WHERE origin = ?1 ORDER BY username")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([origin], |r| {
            Ok(Credential { id: r.get(0)?, origin: r.get(1)?, username: r.get(2)?, updated_at: r.get(3)? })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn browser_credential_save(
    app: AppHandle,
    db: State<crate::db::Db>,
    origin: String,
    username: String,
    password: String,
) -> Result<(), String> {
    if super::origin_of(&format!("{origin}/")).as_deref() != Some(origin.as_str()) {
        return Err("origin must be scheme://host[:port]".into());
    }
    if password.is_empty() {
        return Err("password is empty".into());
    }
    let sealed = seal(&cipher(&app)?, &password)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    upsert(&conn, &origin, &username, &sealed)
}

#[tauri::command(async)]
pub fn browser_credential_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM browser_credentials WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The plaintext, for the "show password" eye in the manager. Deliberately a
/// separate call from the list so a listing never carries secrets.
#[tauri::command(async)]
pub fn browser_credential_reveal(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<String, String> {
    let sealed: String = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT secret FROM browser_credentials WHERE id = ?1", [&id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "that login no longer exists".to_string())?
    };
    open(&cipher(&app)?, &sealed)
}

/// Fill a chosen login into a tab. Refuses unless the tab is currently on the
/// login's origin — a saved GitHub password must never be typed into a page
/// that merely looks like GitHub.
#[tauri::command(async)]
pub fn browser_credential_fill(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    credential_id: String,
) -> Result<(), String> {
    let (origin, username, sealed): (String, String, String) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT origin, username, secret FROM browser_credentials WHERE id = ?1",
            [&credential_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "that login no longer exists".to_string())?
    };
    let password = open(&cipher(&app)?, &sealed)?;
    let script = fill_script(&username, &password);
    super::on_main(&app, move || {
        super::TABS.with(|tabs| {
            let tabs = tabs.borrow();
            let wv = tabs.get(&id).ok_or_else(|| "that browser tab is no longer open".to_string())?;
            let current = wv.url().map_err(|e| e.to_string())?;
            if super::origin_of(&current).as_deref() != Some(origin.as_str()) {
                return Err("the tab is not on that login's site".to_string());
            }
            wv.evaluate_script(&script).map_err(|e| e.to_string())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open_round_trip_and_differ_per_call() {
        let key: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let c = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let a = seal(&c, "hunter2").unwrap();
        let b = seal(&c, "hunter2").unwrap();
        assert_ne!(a, b, "fresh nonce every time");
        assert_eq!(open(&c, &a).unwrap(), "hunter2");
        assert_eq!(open(&c, &b).unwrap(), "hunter2");
    }

    #[test]
    fn wrong_key_or_damaged_blob_is_an_error_not_garbage() {
        let k1: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let k2: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let c1 = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k1));
        let c2 = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k2));
        let sealed = seal(&c1, "secret").unwrap();
        assert!(open(&c2, &sealed).is_err());
        assert!(open(&c1, "not base64!").is_err());
        assert!(open(&c1, "AAAA").is_err());
    }

    #[test]
    fn key_file_is_created_once_and_reused() {
        let dir = std::env::temp_dir().join(format!("devspro-key-{}", uuid::Uuid::new_v4()));
        let path = dir.join("browser").join("credentials.key");
        let a = load_or_create_key(&path).unwrap();
        let b = load_or_create_key(&path).unwrap();
        assert_eq!(a, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn upsert_replaces_the_secret_for_the_same_login() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::MIGRATION_BROWSER).unwrap();
        upsert(&conn, "https://a.test", "me", "one").unwrap();
        upsert(&conn, "https://a.test", "me", "two").unwrap();
        upsert(&conn, "https://a.test", "you", "three").unwrap();
        assert_eq!(stored_secret(&conn, "https://a.test", "me").unwrap().as_deref(), Some("two"));
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM browser_credentials", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn ipc_messages_of_other_kinds_are_ignored() {
        let parsed: CapturedMessage = serde_json::from_str(r#"{"type":"credential","username":"a","password":"b"}"#).unwrap();
        assert_eq!(parsed.kind, "credential");
        assert!(serde_json::from_str::<CapturedMessage>("nope").is_err());
    }

    #[test]
    fn fill_script_escapes_values() {
        let s = fill_script("a\"b", "c'</script>");
        assert!(s.contains(r#"var u = "a\"b""#));
        assert!(s.contains(r#"p = "c'</script>""#));
    }
}
