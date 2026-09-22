//! Env Diff — file IO only.
//!
//! Parsing and diffing live in the frontend, next to the rest of the UI logic and
//! its tests. This side exists because the webview has no filesystem access (the
//! app ships no `tauri-plugin-fs`), the same reason compose files are read here.

use std::fs;
use std::path::Path;

/// Env files are a few KB. The cap stops a mis-picked log or binary from being
/// shipped across the IPC boundary and dumped into a textarea.
const MAX_BYTES: u64 = 1024 * 1024;

fn regular_file(path: &str) -> Result<fs::Metadata, String> {
    let meta = fs::metadata(Path::new(path)).map_err(|e| format!("could not read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a file"));
    }
    Ok(meta)
}

#[tauri::command]
pub fn env_diff_read_file(path: String) -> Result<String, String> {
    let meta = regular_file(&path)?;
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "{path} is {} KB — too large to be an env file (limit {} KB)",
            meta.len() / 1024,
            MAX_BYTES / 1024
        ));
    }
    let bytes = fs::read(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not valid UTF-8 text"))
}

/// Overwrites a file that already exists. It never creates one: the only paths
/// that reach here came from `env_diff_read_file`, so a typo'd or stale path
/// should fail rather than quietly drop a new file somewhere.
///
/// Written in place (not via temp file + rename) so the file keeps its
/// permissions — `.env` files are commonly 0600 — and no `.bak` is left behind,
/// since an unignored `.env.bak` full of secrets is how they end up committed.
#[tauri::command]
pub fn env_diff_write_file(path: String, contents: String) -> Result<String, String> {
    regular_file(&path)?;
    fs::write(&path, contents).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(format!("Saved {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("devspro-env-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn reads_a_text_file() {
        let path = scratch(".env");
        fs::write(&path, "A=1\n").unwrap();
        assert_eq!(env_diff_read_file(path).unwrap(), "A=1\n");
    }

    #[test]
    fn rejects_a_directory() {
        let path = scratch("x");
        let dir = Path::new(&path).parent().unwrap().to_string_lossy().into_owned();
        assert!(env_diff_read_file(dir).unwrap_err().contains("not a file"));
    }

    #[test]
    fn rejects_an_oversized_file() {
        let path = scratch("big.env");
        fs::write(&path, vec![b'a'; (MAX_BYTES + 1) as usize]).unwrap();
        assert!(env_diff_read_file(path).unwrap_err().contains("too large"));
    }

    #[test]
    fn rejects_non_utf8() {
        let path = scratch("bin.env");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(env_diff_read_file(path).unwrap_err().contains("UTF-8"));
    }

    #[test]
    fn write_overwrites_but_never_creates() {
        let path = scratch(".env");
        assert!(env_diff_write_file(path.clone(), "A=1".into()).is_err());
        assert!(!Path::new(&path).exists());

        fs::write(&path, "old").unwrap();
        env_diff_write_file(path.clone(), "A=1\n".into()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "A=1\n");
    }
}
