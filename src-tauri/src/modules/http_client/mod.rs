//! HTTP client — a lightweight Postman-style tab for poking at the sites this
//! app hosts (and anything else). The request goes out from the Rust side, not
//! the webview, for two reasons: the webview's `fetch` is bound by CORS and
//! would refuse most cross-origin calls, and it hides the raw response headers.
//!
//! TLS trust is the system store (native-tls) plus the app's own root CA. The
//! nginx_ssl module installs that CA system-wide when the user asks, but a
//! fresh machine — or one where the install was declined — still has it on
//! disk, so it is added explicitly as well. Either way `https://<site>/` must
//! Just Work, which is the whole reason this module exists.
//!
//! History is a plain append-only log in SQLite; "save" is what the frontend
//! calls after each send, and the user prunes it from the UI.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

/// Appended to `db::later_migrations()`; never reordered.
pub const MIGRATION_HTTP_REQUESTS: &str = r#"
    CREATE TABLE http_requests (
        id TEXT PRIMARY KEY,
        name TEXT,
        method TEXT NOT NULL,
        url TEXT NOT NULL,
        headers TEXT NOT NULL,
        body TEXT,
        created_at TEXT NOT NULL
    );
"#;

/// Beyond this the body is cut and `truncated` set. Anything larger is not
/// something to read in a code block anyway, and shipping tens of megabytes
/// through the IPC bridge as one JSON string would freeze the webview.
pub const BODY_CAP_BYTES: usize = 2 * 1024 * 1024;

/// Upper bound on the caller-supplied timeout so a typo can't hang a send
/// forever; the UI has no cancel button.
const MAX_TIMEOUT_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub truncated: bool,
    pub elapsed_ms: u64,
    /// Full body length as received, before the cap.
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestRecord {
    pub id: String,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// "Key: value" -> ("Key", "value"). Lines without a colon, or with an empty
/// key, are `None` so a raw-headers textarea can drop them silently instead of
/// failing the whole request on a stray blank line.
pub fn parse_header_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), value.trim().to_string()))
}

pub fn parse_header_lines(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(parse_header_line).collect()
}

/// Header pairs from the UI, with blank keys dropped and everything trimmed.
/// A trailing empty row is how the editor offers "add another", so it must not
/// turn into a request error.
pub fn clean_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Whether a Content-Type names JSON — `application/json`, `application/
/// problem+json`, `text/json; charset=utf-8` all count. Used for the same
/// decision on both sides: the UI pretty-prints, and tests pin the rule.
pub fn is_json_content_type(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    mime == "application/json" || mime == "text/json" || mime.ends_with("+json")
}

/// Cheap "would pretty-printing help" check: real JSON is an object or array,
/// and a scalar has nothing to indent. Not a validator — that is serde's job.
pub fn looks_like_json(body: &str) -> bool {
    let t = body.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

/// Pretty-prints JSON, or `None` if it does not parse. The result keeps key
/// order (serde_json's default map is insertion-ordered only with the
/// `preserve_order` feature, so the caller should expect sorted keys).
pub fn pretty_json(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Lossy-decodes at most `cap` bytes, never splitting a multi-byte character
/// in a way that shows up as garbage at the cut.
pub fn cap_body(bytes: &[u8], cap: usize) -> (String, bool) {
    if bytes.len() <= cap {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = cap;
    // Walk back to a char boundary so the lossy decode doesn't invent a
    // replacement character for a character that is only cut, not broken.
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

pub fn parse_method(method: &str) -> Result<reqwest::Method, String> {
    let upper = method.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return Err("method is required".into());
    }
    reqwest::Method::from_bytes(upper.as_bytes())
        .map_err(|_| format!("'{method}' is not a valid HTTP method"))
}

/// `example.test/` -> `http://example.test/`. People paste hostnames, and a
/// bare host is a far more likely intent than a relative URL.
pub fn normalize_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL is required".into());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    reqwest::Url::parse(&with_scheme)
        .map(|u| u.to_string())
        .map_err(|e| format!("'{trimmed}' is not a valid URL: {e}"))
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    for (k, v) in clean_headers(headers) {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| format!("'{k}' is not a valid header name"))?;
        let value =
            HeaderValue::from_str(&v).map_err(|_| format!("header '{k}' has an invalid value"))?;
        map.append(name, value);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

fn build_client(
    follow_redirects: bool,
    timeout_ms: u64,
    extra_root_pem: Option<&Path>,
) -> Result<reqwest::Client, String> {
    let timeout = Duration::from_millis(timeout_ms.clamp(1, MAX_TIMEOUT_MS));
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("DevsPro/", env!("CARGO_PKG_VERSION")))
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
    if let Some(pem_path) = extra_root_pem {
        // Missing or unreadable is not an error: no CA means no local HTTPS
        // sites to trust, and the request may not be to one anyway.
        if let Ok(pem) = std::fs::read(pem_path) {
            if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                builder = builder.add_root_certificate(cert);
            }
        }
    }
    builder
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

#[tauri::command]
pub async fn http_send(
    app: AppHandle,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    follow_redirects: bool,
    timeout_ms: u64,
) -> Result<HttpResponse, String> {
    let method = parse_method(&method)?;
    let url = normalize_url(&url)?;
    let header_map = header_map(&headers)?;

    let root_pem = super::nginx_ssl::certs_dir(&app)
        .ok()
        .map(|dir| super::nginx_ssl::ca::root_ca_files(&dir).cert);
    let client = build_client(follow_redirects, timeout_ms, root_pem.as_deref())?;

    let mut request = client.request(method, &url).headers(header_map);
    if let Some(body) = body.filter(|b| !b.is_empty()) {
        request = request.body(body);
    }

    let started = Instant::now();
    let response = request.send().await.map_err(|e| describe_error(&e))?;
    let status = response.status();
    let response_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let (body, truncated) = cap_body(&bytes, BODY_CAP_BYTES);

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers: response_headers,
        body,
        truncated,
        elapsed_ms,
        size_bytes: bytes.len(),
    })
}

/// reqwest's Display chains the whole cause list into one line, which is the
/// most useful form here; we only add a hint for the two failures a local
/// developer hits most.
fn describe_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "request timed out".into();
    }
    if e.is_connect() {
        return format!("could not connect: {e}");
    }
    e.to_string()
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<HttpRequestRecord> {
    let headers_json: String = row.get("headers")?;
    Ok(HttpRequestRecord {
        id: row.get("id")?,
        name: row.get("name")?,
        method: row.get("method")?,
        url: row.get("url")?,
        // A corrupt headers cell degrades to "no headers" rather than hiding
        // the whole entry — the URL is the part people come back for.
        headers: serde_json::from_str(&headers_json).unwrap_or_default(),
        body: row.get("body")?,
        created_at: row.get("created_at")?,
    })
}

#[tauri::command(async)]
pub fn http_history_list(
    db: State<crate::db::Db>,
    limit: Option<u32>,
) -> Result<Vec<HttpRequestRecord>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100).clamp(1, 1000);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, method, url, headers, body, created_at FROM http_requests
             ORDER BY created_at DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit], row_to_record)
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command(async)]
pub fn http_history_save(
    db: State<crate::db::Db>,
    name: Option<String>,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<HttpRequestRecord, String> {
    let record = HttpRequestRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.filter(|n| !n.trim().is_empty()),
        method: parse_method(&method)?.to_string(),
        url: url.trim().to_string(),
        headers: clean_headers(&headers),
        body: body.filter(|b| !b.is_empty()),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let headers_json = serde_json::to_string(&record.headers).map_err(|e| e.to_string())?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO http_requests (id, name, method, url, headers, body, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            record.id,
            record.name,
            record.method,
            record.url,
            headers_json,
            record.body,
            record.created_at
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(record)
}

#[tauri::command(async)]
pub fn http_history_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM http_requests WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn http_history_clear(db: State<crate::db::Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM http_requests", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_line_parsing_trims_and_splits_on_first_colon() {
        assert_eq!(
            parse_header_line("  Accept : application/json "),
            Some(("Accept".into(), "application/json".into()))
        );
        assert_eq!(
            parse_header_line("Authorization: Bearer a:b:c"),
            Some(("Authorization".into(), "Bearer a:b:c".into()))
        );
        assert_eq!(parse_header_line("no colon here"), None);
        assert_eq!(parse_header_line(": value"), None);
        assert_eq!(parse_header_line(""), None);
    }

    #[test]
    fn header_lines_skip_junk() {
        let parsed = parse_header_lines("A: 1\n\ngarbage\nB:2\n");
        assert_eq!(parsed, vec![("A".into(), "1".into()), ("B".into(), "2".into())]);
    }

    #[test]
    fn clean_headers_drops_blank_keys() {
        let raw = vec![
            (" X-One ".to_string(), " 1 ".to_string()),
            ("".to_string(), "orphan".to_string()),
            ("X-Two".to_string(), "".to_string()),
        ];
        assert_eq!(
            clean_headers(&raw),
            vec![("X-One".into(), "1".into()), ("X-Two".into(), "".into())]
        );
    }

    #[test]
    fn json_content_type_detection() {
        assert!(is_json_content_type("application/json"));
        assert!(is_json_content_type("Application/JSON; charset=utf-8"));
        assert!(is_json_content_type("application/problem+json"));
        assert!(is_json_content_type("text/json"));
        assert!(!is_json_content_type("text/html; charset=utf-8"));
        assert!(!is_json_content_type("application/jsonp"));
        assert!(!is_json_content_type(""));
    }

    #[test]
    fn json_shape_detection() {
        assert!(looks_like_json(" {\"a\":1} "));
        assert!(looks_like_json("[1,2]"));
        assert!(!looks_like_json("42"));
        assert!(!looks_like_json("<html>"));
        assert!(!looks_like_json(""));
    }

    #[test]
    fn pretty_json_round_trips_and_rejects_junk() {
        assert_eq!(pretty_json("{\"a\":[1,2]}").unwrap(), "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
        assert!(pretty_json("{nope").is_none());
    }

    #[test]
    fn cap_body_respects_char_boundaries() {
        let (s, truncated) = cap_body("héllo".as_bytes(), 2);
        // 'h' is 1 byte, 'é' is 2 — cutting at 2 would split 'é'.
        assert_eq!(s, "h");
        assert!(truncated);
        let (s, truncated) = cap_body(b"abc", 10);
        assert_eq!(s, "abc");
        assert!(!truncated);
    }

    #[test]
    fn method_parsing_is_case_insensitive_and_validates() {
        assert_eq!(parse_method("get").unwrap(), reqwest::Method::GET);
        assert_eq!(parse_method(" PATCH ").unwrap(), reqwest::Method::PATCH);
        assert!(parse_method("").is_err());
        assert!(parse_method("GE T").is_err());
    }

    #[test]
    fn url_normalisation_adds_scheme() {
        assert_eq!(normalize_url("example.test/api").unwrap(), "http://example.test/api");
        assert_eq!(normalize_url(" https://a.test ").unwrap(), "https://a.test/");
        assert!(normalize_url("").is_err());
        assert!(normalize_url("http://").is_err());
    }

    #[test]
    fn header_map_rejects_bad_names() {
        assert!(header_map(&[("Bad Name".into(), "x".into())]).is_err());
        assert!(header_map(&[("X-Ok".into(), "x".into())]).is_ok());
    }

    #[test]
    fn migration_creates_table_and_history_round_trips() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION_HTTP_REQUESTS).unwrap();
        conn.execute(
            "INSERT INTO http_requests (id, name, method, url, headers, body, created_at)
             VALUES ('1', NULL, 'GET', 'http://a', '[[\"A\",\"1\"]]', NULL, '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let rec = conn
            .query_row("SELECT * FROM http_requests", [], row_to_record)
            .unwrap();
        assert_eq!(rec.headers, vec![("A".to_string(), "1".to_string())]);
        assert_eq!(rec.name, None);
    }
}
