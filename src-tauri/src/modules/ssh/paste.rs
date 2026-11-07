//! Pasting into a terminal from the clipboard, read here rather than by the
//! webview (see below).
//!
//! An image pasted into a terminal: a terminal carries text alone, so the
//! picture on the clipboard is saved as a PNG and its path typed in instead,
//! which is how Claude Code and other command-line tools take an image. For a
//! remote shell the file is then copied to the server (`sftp_upload_temp`),
//! since a path on this machine means nothing there.
//!
//! The clipboard is read here, not by the webview: a paste event only carries
//! an image on some platforms, a right click has no paste event at all, and
//! this way the webview needs no clipboard permission of its own.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Saved pastes are kept this long: Claude Code reads one when the message
/// goes, which may be a while after the paste.
const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// `devspro-paste-20261101-142530-1a2b3c4d.png`: sorts by time, and the random
/// part keeps two pastes in one second apart, and the name unguessable in a
/// shared `/tmp` on the far end.
pub fn paste_file_name(now: chrono::DateTime<chrono::Local>, random: &str) -> String {
    format!("devspro-paste-{}-{}.png", now.format("%Y%m%d-%H%M%S"), &random[..8.min(random.len())])
}

/// Writes RGBA pixels as a PNG, readable by this user alone: a screenshot can
/// hold anything that was on screen. Never replaces an existing file.
pub fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|e| format!("could not create {}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let failed = |e: png::EncodingError| format!("could not write {}: {e}", path.display());
    let mut writer = encoder.write_header().map_err(failed)?;
    writer.write_image_data(rgba).map_err(failed)?;
    writer.finish().map_err(failed)?;
    Ok(())
}

/// Removes saved pastes older than `KEEP`. Best effort: a file that can't be
/// read or removed is left for next time.
pub fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > KEEP);
        if old && entry.file_name().to_string_lossy().starts_with("devspro-paste-") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn paste_dir() -> Result<PathBuf, String> {
    let dir = super::sftp::cache_root()?.join("devspro").join("pastes");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Saves the image on the clipboard and returns its path; None when the
/// clipboard holds no image, which is the usual case: the terminal asks on
/// every Ctrl+V.
#[tauri::command(async)]
pub fn terminal_clipboard_image(app: AppHandle) -> Result<Option<String>, String> {
    // Text, nothing, or an image in a format the clipboard library can't
    // decode all come back as an error, and all mean there's no image to paste.
    let Ok(image) = app.clipboard().read_image() else {
        return Ok(None);
    };
    let dir = paste_dir()?;
    prune(&dir, SystemTime::now());
    let path = dir.join(paste_file_name(chrono::Local::now(), &uuid::Uuid::new_v4().simple().to_string()));
    write_png(&path, image.rgba(), image.width(), image.height())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// The clipboard's text, for a right click to paste; None when it holds none.
#[tauri::command(async)]
pub fn terminal_clipboard_text(app: AppHandle) -> Option<String> {
    app.clipboard().read_text().ok().filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-paste-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_sort_by_time_and_carry_eight_random_characters() {
        let at = chrono::Local.with_ymd_and_hms(2026, 11, 1, 14, 25, 30).unwrap();
        assert_eq!(paste_file_name(at, "1a2b3c4d5e6f"), "devspro-paste-20261101-142530-1a2b3c4d.png");
    }

    #[test]
    fn writes_a_png_that_decodes_to_the_same_pixels_and_never_overwrites() {
        let dir = scratch();
        let path = dir.join("devspro-paste-x.png");
        let pixels: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8 * 10).collect();
        write_png(&path, &pixels, 2, 3).unwrap();

        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut out).unwrap();
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(&out[..info.buffer_size()], &pixels[..]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(write_png(&path, &pixels, 2, 3).is_err(), "an existing file is left alone");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prunes_only_old_pastes() {
        let dir = scratch();
        let paste = dir.join("devspro-paste-old.png");
        let other = dir.join("notes.txt");
        fs::File::create(&paste).unwrap().write_all(b"x").unwrap();
        fs::File::create(&other).unwrap().write_all(b"x").unwrap();

        prune(&dir, SystemTime::now());
        assert!(paste.exists(), "a fresh paste stays");

        prune(&dir, SystemTime::now() + KEEP + Duration::from_secs(60));
        assert!(!paste.exists(), "an old paste goes");
        assert!(other.exists(), "anything else in the folder is not ours to remove");
        let _ = fs::remove_dir_all(dir);
    }
}
