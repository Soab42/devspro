//! Serves local files to the webview for the File Manager's image, audio,
//! video and PDF tabs, over the custom `fmfile://` scheme.
//!
//! IPC is the wrong pipe for these: a film would have to cross as one base64
//! string before the first frame shows. A URI scheme lets `<video>` and
//! `<img>` load straight from disk, and answering byte ranges is what lets
//! WebKit's media player seek without reading the whole file.
//!
//! Every URL carries a secret made fresh each launch, so only pages that
//! asked the app for one (through `fm_media_token`) can name a file; a page
//! that guesses `fmfile://localhost/etc/passwd` gets a 403. The scheme is
//! registered on the app's own webviews only — the Browser module's tabs are
//! plain wry views that never see it.

use super::gvfs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::http::{header, Method, Request, Response, StatusCode};

pub const SCHEME: &str = "fmfile";

/// The most one ranged response carries. WebKit asks again for the next part,
/// so a film streams in pieces this size instead of being read up front.
const RANGE_CHUNK: u64 = 2 * 1024 * 1024;

/// A request with no Range header gets the whole file — images, PDFs —
/// up to this size; past it, the first chunk as a ranged reply.
const MAX_WHOLE: u64 = 256 * 1024 * 1024;

fn secret() -> &'static str {
    static SECRET: OnceLock<String> = OnceLock::new();
    SECRET.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

/// The secret that prefixes every `fmfile://` path this launch.
#[tauri::command]
pub fn fm_media_token() -> String {
    secret().to_string()
}

/// For the URI scheme: answers on a thread of its own, since a file on a
/// phone or a Drive mount can take a while to read.
pub fn respond(request: Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    std::thread::spawn(move || responder.respond(handle(&request, secret())));
}

fn handle(request: &Request<Vec<u8>>, token: &str) -> Response<Vec<u8>> {
    match serve(request, token) {
        Ok(response) => response,
        Err((status, message)) => Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(message.into_bytes())
            .unwrap_or_default(),
    }
}

type Failure = (StatusCode, String);

/// `/<secret>%2Fhome%2Fme%2Fclip.mp4` (the whole tail percent-encoded, as
/// `convertFileSrc` writes it) -> `/home/me/clip.mp4`, if the secret matches.
fn path_from(uri_path: &str, token: &str) -> Result<PathBuf, Failure> {
    let decoded = percent_decode(uri_path.trim_start_matches('/'))
        .ok_or((StatusCode::BAD_REQUEST, "malformed path".to_string()))?;
    let (given, rest) = decoded.split_once('/').unwrap_or((decoded.as_str(), ""));
    if !same_secret(given, token) {
        return Err((StatusCode::FORBIDDEN, "forbidden".into()));
    }
    if rest.is_empty() {
        return Err((StatusCode::NOT_FOUND, "no file named".into()));
    }
    Ok(PathBuf::from(format!("/{rest}")))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Compares without stopping at the first difference.
fn same_secret(given: &str, token: &str) -> bool {
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// `bytes=START-END`, `bytes=START-` and `bytes=-SUFFIX`, clamped to the file;
/// an inclusive pair. None when unsatisfiable. Only the first of several
/// ranges is honoured, which every media player is fine with.
fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

/// What the webview is told the file is. The extension, and for a file on
/// Google Drive (named by ID on the bridge) the extension of its title.
fn content_type(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mime = super::mime_for(&name, false);
    if mime == "application/octet-stream" || mime == "text/plain" {
        if gvfs::names_by_id(path) {
            if let Some(title) = gvfs::display_name(path) {
                return super::mime_for(&title.display, false);
            }
        }
    }
    mime
}

fn serve(request: &Request<Vec<u8>>, token: &str) -> Result<Response<Vec<u8>>, Failure> {
    let path = path_from(request.uri().path(), token)?;
    let not_found = |e: std::io::Error| (StatusCode::NOT_FOUND, format!("could not open {}: {e}", path.display()));
    let mut file = File::open(&path).map_err(not_found)?;
    let meta = file.metadata().map_err(not_found)?;
    if !meta.is_file() {
        return Err((StatusCode::NOT_FOUND, format!("{} is not a file", path.display())));
    }
    let len = meta.len();
    let head = request.method() == Method::HEAD;
    let range = request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).map(|v| parse_range(v, len));

    let builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type(&path))
        .header(header::ACCEPT_RANGES, "bytes")
        // Same URL after the file is saved over; never show the old bytes.
        .header(header::CACHE_CONTROL, "no-cache");
    let (status, start, end) = match range {
        Some(None) => {
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        Some(Some((start, end))) => (StatusCode::PARTIAL_CONTENT, start, end.min(start + RANGE_CHUNK - 1)),
        None if len > MAX_WHOLE => (StatusCode::PARTIAL_CONTENT, 0, RANGE_CHUNK - 1),
        None => (StatusCode::OK, 0, len.saturating_sub(1)),
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    let mut body = Vec::new();
    if !head && count > 0 {
        file.seek(SeekFrom::Start(start)).map_err(not_found)?;
        body.reserve(count as usize);
        file.take(count).read_to_end(&mut body).map_err(not_found)?;
    }
    let mut builder = builder.status(status).header(header::CONTENT_LENGTH, count);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    builder.body(body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn request(uri: &str, range: Option<&str>) -> Request<Vec<u8>> {
        let mut b = Request::builder().uri(uri);
        if let Some(r) = range {
            b = b.header(header::RANGE, r);
        }
        b.body(Vec::new()).unwrap()
    }

    fn encode(path: &str) -> String {
        path.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    }

    fn scratch_file(bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-media-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("clip one.mp4");
        fs::write(&file, bytes).unwrap();
        file
    }

    #[test]
    fn ranges_parse_and_clamp_to_the_file() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=0-1, 5-6", 100), Some((0, 1)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-1", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }

    #[test]
    fn serves_whole_files_and_ranges_only_with_the_secret() {
        let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let file = scratch_file(&bytes);
        let token = "s3cret";
        let uri = format!("fmfile://localhost/{}", encode(&format!("{token}{}", file.display())));

        let whole = handle(&request(&uri, None), token);
        assert_eq!(whole.status(), StatusCode::OK);
        assert_eq!(whole.body(), &bytes);
        assert_eq!(whole.headers()[header::CONTENT_TYPE], "video/mp4");

        let part = handle(&request(&uri, Some("bytes=10-19")), token);
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.body(), &bytes[10..20]);
        assert_eq!(part.headers()[header::CONTENT_RANGE], "bytes 10-19/1000");

        let past = handle(&request(&uri, Some("bytes=5000-")), token);
        assert_eq!(past.status(), StatusCode::RANGE_NOT_SATISFIABLE);

        let forged = format!("fmfile://localhost/{}", encode(&format!("guess{}", file.display())));
        assert_eq!(handle(&request(&forged, None), token).status(), StatusCode::FORBIDDEN);

        let missing = format!("fmfile://localhost/{}", encode(&format!("{token}/no/such/file.png")));
        assert_eq!(handle(&request(&missing, None), token).status(), StatusCode::NOT_FOUND);
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }
}
