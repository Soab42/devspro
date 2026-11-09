//! Local profile + lock-screen auth. This is a UX/privacy convenience for a shared
//! machine (quick-lock, profile switching) — NOT the app's real security boundary.
//! Anyone with OS-level access to this user account can already read the same
//! SQLite file. The real security boundary is the OS elevation prompt in
//! `crate::elevate` that gates actual system writes.
//!
//! The password is optional. A profile made without one stores an empty
//! `password_hash` and opens without asking — for a personal machine already
//! behind its OS login, a second password is only friction. (Unlocking with
//! the OS login itself, and resetting a forgotten password, are not done yet:
//! see GitHub issue #1.)

pub mod model;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use model::Profile;
use rusqlite::Connection;

pub fn migrations() -> Vec<&'static str> {
    vec![
        r#"
        CREATE TABLE profiles (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    ]
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("failed to hash password: {e}"))
}

fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// The columns `row_to_profile` reads, in its order.
const PROFILE_COLUMNS: &str = "id, display_name, created_at, password_hash <> ''";

fn row_to_profile(row: &rusqlite::Row) -> rusqlite::Result<Profile> {
    Ok(Profile {
        id: row.get(0)?,
        display_name: row.get(1)?,
        created_at: row.get(2)?,
        has_password: row.get(3)?,
    })
}

fn list_profiles(conn: &Connection) -> rusqlite::Result<Vec<Profile>> {
    let mut stmt = conn.prepare(&format!("SELECT {PROFILE_COLUMNS} FROM profiles ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_profile)?;
    rows.collect()
}

/// The name a profile made without one gets: the OS account's, so the
/// profile menu still says whose it is.
fn default_display_name() -> String {
    ["USERNAME", "USER", "LOGNAME"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(|name| name.trim().to_string())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "Me".to_string())
}

/// Creates a profile and makes it the open session. An empty name becomes
/// the OS account's; an empty password means none.
fn create_profile(conn: &Connection, display_name: &str, password: &str) -> Result<Profile, String> {
    let display_name = match display_name.trim() {
        "" => default_display_name(),
        name => name.to_string(),
    };
    let password_hash = if password.is_empty() {
        String::new()
    } else if password.chars().count() < 4 {
        return Err("password must be at least 4 characters (or leave it empty for none)".to_string());
    } else {
        hash_password(password)?
    };

    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        display_name,
        created_at: chrono::Utc::now().to_rfc3339(),
        has_password: !password_hash.is_empty(),
    };
    conn.execute(
        "INSERT INTO profiles (id, display_name, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
        (&profile.id, &profile.display_name, &password_hash, &profile.created_at),
    )
    .map_err(|e| e.to_string())?;
    persist_session(conn, &profile.id).map_err(|e| e.to_string())?;
    Ok(profile)
}

/// Opens `profile_id`: with its password, or with nothing when it has none.
fn unlock_profile(conn: &Connection, profile_id: &str, password: &str) -> Result<Profile, String> {
    let (hash, profile): (String, Profile) = conn
        .query_row(
            &format!("SELECT password_hash, {PROFILE_COLUMNS} FROM profiles WHERE id = ?1"),
            [profile_id],
            |row| {
                Ok((
                    row.get(0)?,
                    Profile {
                        id: row.get(1)?,
                        display_name: row.get(2)?,
                        created_at: row.get(3)?,
                        has_password: row.get(4)?,
                    },
                ))
            },
        )
        .map_err(|_| "profile not found".to_string())?;

    if hash.is_empty() || verify_password(&hash, password) {
        persist_session(conn, &profile.id).map_err(|e| e.to_string())?;
        Ok(profile)
    } else {
        Err("incorrect password".to_string())
    }
}

#[tauri::command(async)]
pub fn auth_has_profile(db: tauri::State<crate::db::Db>) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

#[tauri::command(async)]
pub fn auth_list_profiles(db: tauri::State<crate::db::Db>) -> Result<Vec<Profile>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_profiles(&conn).map_err(|e| e.to_string())
}

/// `password` may be empty: the profile then has none.
#[tauri::command(async)]
pub fn auth_create_profile(
    db: tauri::State<crate::db::Db>,
    display_name: String,
    password: String,
) -> Result<Profile, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    create_profile(&conn, &display_name, &password)
}

const SESSION_SETTING_KEY: &str = "session_unlocked_profile_id";

/// Restores whichever profile was unlocked last time, if any — the local lock
/// screen is a convenience/profile-switch gate, not this app's real security
/// boundary (see module docs), so staying unlocked across restarts until the user
/// explicitly logs out is consistent with that, and saves re-typing a password on
/// every dev-server relaunch.
#[tauri::command(async)]
pub fn auth_current_session(db: tauri::State<crate::db::Db>) -> Result<Option<Profile>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let profile_id: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [SESSION_SETTING_KEY],
            |row| row.get(0),
        )
        .ok();
    let Some(profile_id) = profile_id else {
        return Ok(None);
    };
    let profile = conn
        .query_row(
            &format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?1"),
            [&profile_id],
            row_to_profile,
        )
        .ok();
    Ok(profile)
}

#[tauri::command(async)]
pub fn auth_logout(db: tauri::State<crate::db::Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM settings WHERE key = ?1", [SESSION_SETTING_KEY])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn persist_session(conn: &Connection, profile_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (SESSION_SETTING_KEY, profile_id),
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn auth_unlock(
    db: tauri::State<crate::db::Db>,
    profile_id: String,
    password: String,
) -> Result<Profile, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    unlock_profile(&conn, &profile_id, &password)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for m in migrations() {
            conn.execute_batch(m).unwrap();
        }
        conn
    }

    #[test]
    fn a_profile_without_a_password_opens_without_one() {
        let conn = db();
        let p = create_profile(&conn, "Talha", "").unwrap();
        assert!(!p.has_password);
        assert_eq!(unlock_profile(&conn, &p.id, "").unwrap().id, p.id);
        assert!(unlock_profile(&conn, &p.id, "anything").is_ok());
        assert!(!list_profiles(&conn).unwrap()[0].has_password);
    }

    #[test]
    fn a_profile_with_a_password_needs_it() {
        let conn = db();
        let p = create_profile(&conn, "Talha", "s3cret").unwrap();
        assert!(p.has_password);
        assert!(unlock_profile(&conn, &p.id, "").is_err());
        assert!(unlock_profile(&conn, &p.id, "wrong").is_err());
        assert!(unlock_profile(&conn, &p.id, "s3cret").unwrap().has_password);
    }

    #[test]
    fn a_short_password_is_refused_and_an_empty_name_is_filled_in() {
        let conn = db();
        assert!(create_profile(&conn, "x", "abc").unwrap_err().contains("at least 4"));
        let p = create_profile(&conn, "   ", "").unwrap();
        assert!(!p.display_name.trim().is_empty());
    }
}
