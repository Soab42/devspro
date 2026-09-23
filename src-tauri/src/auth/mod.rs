//! Local profile + lock-screen auth. This is a UX/privacy convenience for a shared
//! machine (quick-lock, profile switching) — NOT the app's real security boundary.
//! Anyone with OS-level access to this user account can already read the same
//! SQLite file. The real security boundary is the OS elevation prompt in
//! `crate::elevate` that gates actual system writes.

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

fn list_profiles(conn: &Connection) -> rusqlite::Result<Vec<Profile>> {
    let mut stmt = conn.prepare("SELECT id, display_name, created_at FROM profiles ORDER BY created_at")?;
    let rows = stmt.query_map([], |row| {
        Ok(Profile {
            id: row.get(0)?,
            display_name: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    rows.collect()
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

#[tauri::command(async)]
pub fn auth_create_profile(
    db: tauri::State<crate::db::Db>,
    display_name: String,
    password: String,
) -> Result<Profile, String> {
    if display_name.trim().is_empty() {
        return Err("display name cannot be empty".to_string());
    }
    if password.len() < 4 {
        return Err("password must be at least 4 characters".to_string());
    }

    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        display_name: display_name.trim().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let password_hash = hash_password(&password)?;

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO profiles (id, display_name, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
        (&profile.id, &profile.display_name, &password_hash, &profile.created_at),
    )
    .map_err(|e| e.to_string())?;
    persist_session(&conn, &profile.id).map_err(|e| e.to_string())?;

    Ok(profile)
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
            "SELECT id, display_name, created_at FROM profiles WHERE id = ?1",
            [&profile_id],
            |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
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
    let (hash, profile): (String, Profile) = conn
        .query_row(
            "SELECT password_hash, id, display_name, created_at FROM profiles WHERE id = ?1",
            [&profile_id],
            |row| {
                Ok((
                    row.get(0)?,
                    Profile {
                        id: row.get(1)?,
                        display_name: row.get(2)?,
                        created_at: row.get(3)?,
                    },
                ))
            },
        )
        .map_err(|_| "profile not found".to_string())?;

    if verify_password(&hash, &password) {
        persist_session(&conn, &profile.id).map_err(|e| e.to_string())?;
        Ok(profile)
    } else {
        Err("incorrect password".to_string())
    }
}
