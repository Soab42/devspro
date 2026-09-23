//! Last-N-lines of a file without reading the whole thing.
//!
//! Every log viewer in the app polls its file every second or two, and the
//! scheduled-tasks list reads one log per task on each refresh. Logs grow
//! without bound, so `read_to_string` + split turned each tick into a full
//! read of a file that can be hundreds of megabytes. This reads backwards from
//! the end in fixed chunks and stops as soon as it has seen enough newlines.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: u64 = 64 * 1024;

/// The last `n` lines of `path`, newline-joined, with no trailing newline.
/// Semantically identical to reading the whole file and keeping the last `n`
/// of `str::lines()`. Bytes that are not valid UTF-8 are replaced.
pub fn tail_file(path: &Path, n: usize) -> std::io::Result<String> {
    if n == 0 {
        return Ok(String::new());
    }
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut end = len;
    let mut acc: Vec<u8> = Vec::new();

    // A trailing newline terminates the last line rather than starting a new
    // empty one, so `n` lines need `n + 1` newlines behind them (or the start
    // of the file). Count while reading so we can stop early.
    let mut newlines = 0usize;
    while end > 0 {
        let start = end.saturating_sub(CHUNK);
        let size = (end - start) as usize;
        let mut chunk = vec![0u8; size];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut chunk)?;
        newlines += chunk.iter().filter(|&&b| b == b'\n').count();
        chunk.extend_from_slice(&acc);
        acc = chunk;
        end = start;
        if newlines > n {
            break;
        }
    }

    let text = String::from_utf8_lossy(&acc);
    Ok(tail_lines(&text, n))
}

/// The last `n` of `contents.lines()`, joined. Shared by the in-memory callers
/// (tests, small strings) and `tail_file`.
pub fn tail_lines(contents: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_file(contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("devspro-tail-{}", uuid::Uuid::new_v4()));
        File::create(&path).unwrap().write_all(contents).unwrap();
        path
    }

    fn check(contents: &str, n: usize) {
        let path = temp_file(contents.as_bytes());
        let got = tail_file(&path, n).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(got, tail_lines(contents, n), "contents={contents:?} n={n}");
    }

    #[test]
    fn matches_the_in_memory_version_on_small_files() {
        for contents in ["", "a", "a\n", "a\nb\nc", "a\nb\nc\n", "\n\n\n", "a\n\nb\n"] {
            for n in [0, 1, 2, 3, 10] {
                check(contents, n);
            }
        }
    }

    #[test]
    fn matches_across_chunk_boundaries() {
        // Lines long enough that the requested tail straddles several 64 KiB
        // chunks, and a file large enough to need more than one read.
        let line = "x".repeat(10_000);
        let contents: String = (0..40).map(|i| format!("{i}:{line}\n")).collect();
        for n in [1, 5, 7, 13, 40, 41] {
            check(&contents, n);
        }
    }

    #[test]
    fn stops_reading_once_it_has_enough() {
        // Not observable directly; a big file with a small tail must simply
        // return the right answer quickly.
        let contents: String = (0..200_000).map(|i| format!("line {i}\n")).collect();
        let path = temp_file(contents.as_bytes());
        let got = tail_file(&path, 2).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(got, "line 199998\nline 199999");
    }

    #[test]
    fn missing_file_is_an_error() {
        let missing = std::env::temp_dir().join(format!("devspro-absent-{}", uuid::Uuid::new_v4()));
        assert!(tail_file(&missing, 5).is_err());
    }
}
