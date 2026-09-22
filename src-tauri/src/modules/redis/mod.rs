//! Redis — a keys and Laravel-queue inspector.
//!
//! Talks RESP directly over a `std::net::TcpStream` rather than pulling in a
//! Redis crate. The handful of commands this page needs (SCAN, TYPE, TTL, the
//! per-type readers, LLEN/ZCARD, INFO) are trivial to encode, the RESP2 wire
//! format is a few dozen lines to decode, and going straight to the socket
//! means it works the same whether Redis runs natively or in Docker with a
//! published port. The codec is pure and unit-tested against `Cursor`s.
//!
//! Every command opens a fresh connection. Connections are cheap on localhost,
//! and holding one open across the IPC boundary would mean managing state,
//! reconnects and stale AUTH for no visible gain on a page that polls at 5s.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// How many collection items a single `redis_get` ships to the webview. A
/// million-entry list is a thing on a busy queue; the page shows a sample and
/// the length.
const ITEM_CAP: usize = 100;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Connection settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConn {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub db: Option<u32>,
}

impl RedisConn {
    fn host(&self) -> &str {
        match self.host.as_deref().map(str::trim) {
            Some("") | None => "127.0.0.1",
            Some(h) => h,
        }
    }
    fn port(&self) -> u16 {
        self.port.unwrap_or(6379)
    }
}

// ---------------------------------------------------------------------------
// RESP codec (pure)
// ---------------------------------------------------------------------------

/// The five RESP2 reply types. `Bulk(None)` and `Array(None)` are the nil
/// replies (`$-1`, `*-1`) — a missing key, or a `LINDEX` past the end.
#[derive(Debug, Clone, PartialEq)]
pub enum Resp {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Option<Vec<u8>>),
    Array(Option<Vec<Resp>>),
}

impl Resp {
    /// Turns a server-side error reply into `Err`, so callers can `?` it.
    fn ok(self) -> Result<Resp, String> {
        match self {
            Resp::Error(e) => Err(friendly_error(&e)),
            other => Ok(other),
        }
    }

    /// A bulk or simple string as UTF-8 (lossy — Redis values are bytes).
    fn text(&self) -> Option<String> {
        match self {
            Resp::Simple(s) => Some(s.clone()),
            Resp::Bulk(Some(b)) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }

    fn int(&self) -> Option<i64> {
        match self {
            Resp::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn items(&self) -> Vec<Resp> {
        match self {
            Resp::Array(Some(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    /// An array of bulk strings as text; anything that isn't text becomes "".
    fn strings(&self) -> Vec<String> {
        self.items()
            .iter()
            .map(|r| r.text().unwrap_or_default())
            .collect()
    }
}

/// Encodes one command as a RESP array of bulk strings — the only request
/// form modern servers require, and the only one that is safe for arbitrary
/// bytes (inline commands would break on spaces and newlines in values).
pub fn encode_command<S: AsRef<[u8]>>(args: &[S]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
    for a in args {
        let a = a.as_ref();
        out.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
        out.extend_from_slice(a);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn read_line<R: BufRead>(r: &mut R) -> Result<String, String> {
    let mut buf = Vec::new();
    let n = r
        .read_until(b'\n', &mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    if n == 0 {
        return Err("connection closed by Redis".into());
    }
    if !buf.ends_with(b"\r\n") {
        return Err("malformed RESP line (missing CRLF)".into());
    }
    buf.truncate(buf.len() - 2);
    String::from_utf8(buf).map_err(|_| "malformed RESP line (not UTF-8)".to_string())
}

/// Decodes exactly one reply. Nested arrays recurse; a bulk string is read as
/// `len + 2` raw bytes so embedded newlines in values are preserved.
pub fn decode_reply<R: BufRead>(r: &mut R) -> Result<Resp, String> {
    let line = read_line(r)?;
    let (kind, rest) = line
        .split_at_checked(1)
        .ok_or_else(|| "empty RESP line".to_string())?;
    match kind {
        "+" => Ok(Resp::Simple(rest.to_string())),
        "-" => Ok(Resp::Error(rest.to_string())),
        ":" => rest
            .parse::<i64>()
            .map(Resp::Int)
            .map_err(|_| format!("malformed RESP integer: {rest}")),
        "$" => {
            let len: i64 = rest
                .parse()
                .map_err(|_| format!("malformed RESP bulk length: {rest}"))?;
            if len < 0 {
                return Ok(Resp::Bulk(None));
            }
            let mut buf = vec![0u8; len as usize + 2];
            r.read_exact(&mut buf)
                .map_err(|e| format!("read failed: {e}"))?;
            if !buf.ends_with(b"\r\n") {
                return Err("malformed RESP bulk string (missing CRLF)".into());
            }
            buf.truncate(len as usize);
            Ok(Resp::Bulk(Some(buf)))
        }
        "*" => {
            let len: i64 = rest
                .parse()
                .map_err(|_| format!("malformed RESP array length: {rest}"))?;
            if len < 0 {
                return Ok(Resp::Array(None));
            }
            let mut items = Vec::with_capacity(len as usize);
            for _ in 0..len {
                items.push(decode_reply(r)?);
            }
            Ok(Resp::Array(Some(items)))
        }
        _ => Err(format!("unexpected RESP type byte {kind:?}")),
    }
}

/// Convenience for tests and for parsing a complete buffer.
pub fn decode_bytes(bytes: &[u8]) -> Result<Resp, String> {
    decode_reply(&mut Cursor::new(bytes))
}

/// Redis error replies are terse (`NOAUTH Authentication required.`); a
/// sentence with the fix is friendlier on a settings-style page.
fn friendly_error(e: &str) -> String {
    if e.starts_with("NOAUTH") {
        "Redis requires a password (NOAUTH). Enter it in the connection bar.".into()
    } else if e.starts_with("WRONGPASS") || e.contains("invalid password") {
        "Redis rejected the password.".into()
    } else if e.starts_with("ERR AUTH") && e.contains("no password is set") {
        "This Redis has no password set — leave the password field empty.".into()
    } else {
        format!("Redis error: {e}")
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(conn: &RedisConn) -> Result<Self, String> {
        let host = conn.host();
        let port = conn.port();
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("could not resolve {host}: {e}"))?
            .next()
            .ok_or_else(|| format!("could not resolve {host}"))?;
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| {
            if e.kind() == std::io::ErrorKind::ConnectionRefused {
                format!("Connection refused at {host}:{port} — is Redis running and is the port published?")
            } else if e.kind() == std::io::ErrorKind::TimedOut {
                format!("Timed out connecting to {host}:{port}")
            } else {
                format!("could not connect to {host}:{port}: {e}")
            }
        })?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
        stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
        let writer = stream
            .try_clone()
            .map_err(|e| format!("could not clone socket: {e}"))?;
        let mut client = Client {
            reader: BufReader::new(stream),
            writer,
        };
        if let Some(pw) = conn.password.as_deref().filter(|p| !p.is_empty()) {
            client.cmd(&["AUTH", pw])?;
        }
        if let Some(db) = conn.db.filter(|d| *d > 0) {
            client.cmd(&["SELECT", &db.to_string()])?;
        }
        Ok(client)
    }

    fn cmd<S: AsRef<[u8]>>(&mut self, args: &[S]) -> Result<Resp, String> {
        self.writer
            .write_all(&encode_command(args))
            .map_err(|e| format!("write failed: {e}"))?;
        decode_reply(&mut self.reader)?.ok()
    }

    /// Sends every command before reading any reply — one round trip for a
    /// page of `TYPE`/`TTL` lookups instead of two per key.
    fn pipeline(&mut self, cmds: &[Vec<String>]) -> Result<Vec<Resp>, String> {
        let mut buf = Vec::new();
        for c in cmds {
            buf.extend_from_slice(&encode_command(c));
        }
        self.writer
            .write_all(&buf)
            .map_err(|e| format!("write failed: {e}"))?;
        let mut out = Vec::with_capacity(cmds.len());
        for _ in cmds {
            out.push(decode_reply(&mut self.reader)?.ok()?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// `INFO` output is `key:value` lines grouped under `# Section` comments.
/// Sections are flattened; no field name repeats across the ones we read.
pub fn parse_info(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum QueueRole {
    Pending,
    Delayed,
    Reserved,
}

/// Recognises Laravel's Redis queue keys. Laravel prefixes every key with
/// `config('database.redis.options.prefix')` (`laravel_database_` by
/// default), so the match is on the `queues:` segment wherever it sits.
/// Returns `(prefix, queue name, role)`; the `:notify` list Laravel uses to
/// wake blocked workers is not a queue and yields `None`.
pub fn parse_queue_key(key: &str) -> Option<(String, String, QueueRole)> {
    let idx = key.find("queues:")?;
    let prefix = &key[..idx];
    let rest = &key[idx + "queues:".len()..];
    if rest.is_empty() {
        return None;
    }
    if let Some(name) = rest.strip_suffix(":delayed") {
        return Some((prefix.into(), name.into(), QueueRole::Delayed));
    }
    if let Some(name) = rest.strip_suffix(":reserved") {
        return Some((prefix.into(), name.into(), QueueRole::Reserved));
    }
    if rest.ends_with(":notify") {
        return None;
    }
    Some((prefix.into(), rest.into(), QueueRole::Pending))
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QueueView {
    pub queue: String,
    /// The full pending-list key, prefix included — what `redis_peek_job` needs.
    pub key: String,
    pub pending: i64,
    pub delayed: i64,
    pub reserved: i64,
}

/// Groups scanned keys into one row per `(prefix, queue)`. A queue whose only
/// key is `:reserved` (everything pending has been picked up) still gets a
/// row, which is why grouping happens before any count is fetched.
pub fn group_queue_keys(keys: &[String]) -> Vec<QueueView> {
    let mut map: BTreeMap<(String, String), QueueView> = BTreeMap::new();
    for key in keys {
        if let Some((prefix, name, _)) = parse_queue_key(key) {
            map.entry((prefix.clone(), name.clone()))
                .or_insert_with(|| QueueView {
                    queue: name.clone(),
                    key: format!("{prefix}queues:{name}"),
                    pending: 0,
                    delayed: 0,
                    reserved: 0,
                });
        }
    }
    map.into_values().collect()
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct JobPeek {
    /// Pretty-printed when the payload is JSON; verbatim otherwise.
    pub raw: String,
    pub display_name: Option<String>,
    pub command_name: Option<String>,
    pub attempts: Option<i64>,
    pub id: Option<String>,
}

/// Laravel's job envelope is `{uuid, displayName, job, data: {commandName,
/// command}, attempts, ...}`. Anything we can't parse still comes back as
/// `raw` so the modal always has something to show.
pub fn summarise_job(payload: &str) -> JobPeek {
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(v) => JobPeek {
            raw: serde_json::to_string_pretty(&v).unwrap_or_else(|_| payload.to_string()),
            display_name: v.get("displayName").and_then(|d| d.as_str()).map(String::from),
            command_name: v
                .get("data")
                .and_then(|d| d.get("commandName"))
                .and_then(|c| c.as_str())
                .map(String::from),
            attempts: v.get("attempts").and_then(|a| a.as_i64()),
            id: v
                .get("uuid")
                .or_else(|| v.get("id"))
                .and_then(|i| i.as_str())
                .map(String::from),
        },
        Err(_) => JobPeek {
            raw: payload.to_string(),
            display_name: None,
            command_name: None,
            attempts: None,
            id: None,
        },
    }
}

/// Full-key SCAN with the cursor loop hidden. Only used for the bounded
/// `*queues:*` / `horizon:*` patterns; the Keys tab pages explicitly.
fn scan_all(client: &mut Client, pattern: &str) -> Result<Vec<String>, String> {
    let mut cursor = "0".to_string();
    let mut keys = Vec::new();
    loop {
        let reply = client.cmd(&["SCAN", &cursor, "MATCH", pattern, "COUNT", "500"])?;
        let items = reply.items();
        cursor = items.first().and_then(Resp::text).unwrap_or_else(|| "0".into());
        if let Some(batch) = items.get(1) {
            keys.extend(batch.strings());
        }
        if cursor == "0" {
            break;
        }
    }
    Ok(keys)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct PingView {
    pub ok: bool,
    pub version: String,
    pub uptime_seconds: i64,
    pub used_memory_human: String,
    pub connected_clients: i64,
    pub total_keys: i64,
    pub db: u32,
}

#[tauri::command]
pub fn redis_ping(conn: RedisConn) -> Result<PingView, String> {
    let mut client = Client::connect(&conn)?;
    client.cmd(&["PING"])?;
    let info = parse_info(&client.cmd(&["INFO"])?.text().unwrap_or_default());
    let total_keys = client.cmd(&["DBSIZE"])?.int().unwrap_or(0);
    let num = |k: &str| info.get(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    Ok(PingView {
        ok: true,
        version: info.get("redis_version").cloned().unwrap_or_else(|| "?".into()),
        uptime_seconds: num("uptime_in_seconds"),
        used_memory_human: info.get("used_memory_human").cloned().unwrap_or_default(),
        connected_clients: num("connected_clients"),
        total_keys,
        db: conn.db.unwrap_or(0),
    })
}

#[derive(Debug, Serialize)]
pub struct KeyView {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// -1 = no expiry, -2 = vanished between SCAN and TTL.
    pub ttl: i64,
}

#[derive(Debug, Serialize)]
pub struct ScanView {
    pub keys: Vec<KeyView>,
    /// "0" once the iteration has wrapped.
    pub cursor: String,
}

#[tauri::command]
pub fn redis_scan(
    conn: RedisConn,
    pattern: String,
    cursor: String,
    count: Option<u32>,
) -> Result<ScanView, String> {
    let mut client = Client::connect(&conn)?;
    let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
    let count = count.unwrap_or(100).clamp(10, 1000).to_string();
    let cursor = if cursor.trim().is_empty() { "0" } else { cursor.trim() };
    let reply = client.cmd(&["SCAN", cursor, "MATCH", pattern, "COUNT", &count])?;
    let items = reply.items();
    let next = items.first().and_then(Resp::text).unwrap_or_else(|| "0".into());
    let names = items.get(1).map(Resp::strings).unwrap_or_default();

    let cmds: Vec<Vec<String>> = names
        .iter()
        .flat_map(|k| {
            [
                vec!["TYPE".to_string(), k.clone()],
                vec!["TTL".to_string(), k.clone()],
            ]
        })
        .collect();
    let replies = client.pipeline(&cmds)?;
    let keys = names
        .into_iter()
        .zip(replies.chunks(2))
        .map(|(key, pair)| KeyView {
            key,
            kind: pair.first().and_then(Resp::text).unwrap_or_else(|| "none".into()),
            ttl: pair.get(1).and_then(Resp::int).unwrap_or(-1),
        })
        .collect();
    Ok(ScanView { keys, cursor: next })
}

#[derive(Debug, Serialize)]
pub struct ZMember {
    pub member: String,
    pub score: f64,
}

/// One variant per Redis type the page knows how to render. `length` is the
/// real size; `items`/`members` are capped at [`ITEM_CAP`].
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ValueView {
    String { value: String },
    List { items: Vec<String>, length: i64 },
    Hash { fields: Vec<(String, String)>, length: i64 },
    Set { members: Vec<String>, length: i64 },
    ZSet { members: Vec<ZMember>, length: i64 },
    None {},
    Other { kind: String },
}

fn pairs(flat: Vec<String>) -> Vec<(String, String)> {
    flat.chunks(2)
        .map(|c| (c[0].clone(), c.get(1).cloned().unwrap_or_default()))
        .collect()
}

#[tauri::command]
pub fn redis_get(conn: RedisConn, key: String) -> Result<ValueView, String> {
    let mut client = Client::connect(&conn)?;
    let kind = client.cmd(&["TYPE", &key])?.text().unwrap_or_default();
    let last = (ITEM_CAP - 1).to_string();
    let cap = ITEM_CAP.to_string();
    Ok(match kind.as_str() {
        "string" => ValueView::String {
            value: client.cmd(&["GET", &key])?.text().unwrap_or_default(),
        },
        "list" => {
            let r = client.pipeline(&[
                vec!["LRANGE".into(), key.clone(), "0".into(), last],
                vec!["LLEN".into(), key.clone()],
            ])?;
            ValueView::List {
                items: r[0].strings(),
                length: r[1].int().unwrap_or(0),
            }
        }
        "hash" => {
            let flat = client.cmd(&["HGETALL", &key])?.strings();
            let length = (flat.len() / 2) as i64;
            let mut fields = pairs(flat);
            fields.truncate(ITEM_CAP);
            ValueView::Hash { fields, length }
        }
        "set" => {
            let r = client.pipeline(&[
                vec!["SSCAN".into(), key.clone(), "0".into(), "COUNT".into(), cap],
                vec!["SCARD".into(), key.clone()],
            ])?;
            let mut members = r[0].items().get(1).map(Resp::strings).unwrap_or_default();
            members.truncate(ITEM_CAP);
            ValueView::Set {
                members,
                length: r[1].int().unwrap_or(0),
            }
        }
        "zset" => {
            let r = client.pipeline(&[
                vec!["ZRANGE".into(), key.clone(), "0".into(), last, "WITHSCORES".into()],
                vec!["ZCARD".into(), key.clone()],
            ])?;
            let members = pairs(r[0].strings())
                .into_iter()
                .map(|(member, score)| ZMember {
                    member,
                    score: score.parse().unwrap_or(0.0),
                })
                .collect();
            ValueView::ZSet {
                members,
                length: r[1].int().unwrap_or(0),
            }
        }
        "none" => ValueView::None {},
        other => ValueView::Other { kind: other.to_string() },
    })
}

#[tauri::command]
pub fn redis_delete(conn: RedisConn, keys: Vec<String>) -> Result<i64, String> {
    if keys.is_empty() {
        return Ok(0);
    }
    let mut client = Client::connect(&conn)?;
    let mut args = vec!["DEL".to_string()];
    args.extend(keys);
    Ok(client.cmd(&args)?.int().unwrap_or(0))
}

/// Destructive; the page confirms first. Only the selected db is flushed —
/// never FLUSHALL.
#[tauri::command]
pub fn redis_flush_db(conn: RedisConn) -> Result<String, String> {
    let mut client = Client::connect(&conn)?;
    client.cmd(&["FLUSHDB"])?;
    Ok(format!("Flushed db {}", conn.db.unwrap_or(0)))
}

#[derive(Debug, Serialize)]
pub struct QueuesView {
    pub queues: Vec<QueueView>,
    /// Horizon keeps its own bookkeeping under `horizon:*`; the count is a
    /// cheap "Horizon is in use" signal without decoding its structures.
    pub horizon_keys: usize,
}

#[tauri::command]
pub fn redis_laravel_queues(conn: RedisConn) -> Result<QueuesView, String> {
    let mut client = Client::connect(&conn)?;
    let keys = scan_all(&mut client, "*queues:*")?;
    let mut queues = group_queue_keys(&keys);
    let cmds: Vec<Vec<String>> = queues
        .iter()
        .flat_map(|q| {
            [
                vec!["LLEN".to_string(), q.key.clone()],
                vec!["ZCARD".to_string(), format!("{}:delayed", q.key)],
                vec!["ZCARD".to_string(), format!("{}:reserved", q.key)],
            ]
        })
        .collect();
    // LLEN on a missing key is 0; on a key of the wrong type it's WRONGTYPE,
    // which pipeline() turns into Err — acceptable, since a `queues:` key that
    // isn't a list isn't Laravel's.
    let replies = client.pipeline(&cmds)?;
    for (q, r) in queues.iter_mut().zip(replies.chunks(3)) {
        q.pending = r.first().and_then(Resp::int).unwrap_or(0);
        q.delayed = r.get(1).and_then(Resp::int).unwrap_or(0);
        q.reserved = r.get(2).and_then(Resp::int).unwrap_or(0);
    }
    let horizon_keys = scan_all(&mut client, "*horizon:*")?.len();
    Ok(QueuesView { queues, horizon_keys })
}

/// `queue` is the full pending-list key (`QueueView::key`); a bare name is
/// accepted too for the unprefixed default.
#[tauri::command]
pub fn redis_peek_job(conn: RedisConn, queue: String, index: Option<i64>) -> Result<JobPeek, String> {
    let key = if queue.contains("queues:") {
        queue
    } else {
        format!("queues:{queue}")
    };
    let mut client = Client::connect(&conn)?;
    let idx = index.unwrap_or(0).to_string();
    match client.cmd(&["LINDEX", &key, &idx])? {
        Resp::Bulk(Some(b)) => Ok(summarise_job(&String::from_utf8_lossy(&b))),
        _ => Err(format!("No job at position {idx} in {key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_command_as_bulk_array() {
        assert_eq!(
            encode_command(&["SET", "k", "v v"]),
            b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$3\r\nv v\r\n".to_vec()
        );
    }

    #[test]
    fn decodes_simple_error_and_integer() {
        assert_eq!(decode_bytes(b"+PONG\r\n").unwrap(), Resp::Simple("PONG".into()));
        assert_eq!(
            decode_bytes(b"-ERR nope\r\n").unwrap(),
            Resp::Error("ERR nope".into())
        );
        assert_eq!(decode_bytes(b":-42\r\n").unwrap(), Resp::Int(-42));
    }

    #[test]
    fn decodes_bulk_strings_including_nil_and_embedded_newlines() {
        assert_eq!(
            decode_bytes(b"$4\r\na\r\nb\r\n").unwrap(),
            Resp::Bulk(Some(b"a\r\nb".to_vec()))
        );
        assert_eq!(decode_bytes(b"$0\r\n\r\n").unwrap(), Resp::Bulk(Some(vec![])));
        assert_eq!(decode_bytes(b"$-1\r\n").unwrap(), Resp::Bulk(None));
    }

    #[test]
    fn decodes_nested_arrays() {
        let scan = b"*2\r\n$1\r\n0\r\n*2\r\n$3\r\nfoo\r\n:7\r\n";
        assert_eq!(
            decode_bytes(scan).unwrap(),
            Resp::Array(Some(vec![
                Resp::Bulk(Some(b"0".to_vec())),
                Resp::Array(Some(vec![Resp::Bulk(Some(b"foo".to_vec())), Resp::Int(7)])),
            ]))
        );
        assert_eq!(decode_bytes(b"*-1\r\n").unwrap(), Resp::Array(None));
        assert_eq!(decode_bytes(b"*0\r\n").unwrap(), Resp::Array(Some(vec![])));
    }

    #[test]
    fn decodes_several_replies_in_sequence() {
        let mut cur = Cursor::new(b"+OK\r\n:1\r\n$2\r\nhi\r\n".to_vec());
        assert_eq!(decode_reply(&mut cur).unwrap(), Resp::Simple("OK".into()));
        assert_eq!(decode_reply(&mut cur).unwrap(), Resp::Int(1));
        assert_eq!(decode_reply(&mut cur).unwrap(), Resp::Bulk(Some(b"hi".to_vec())));
        assert!(decode_reply(&mut cur).unwrap_err().contains("closed"));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(decode_bytes(b"?x\r\n").unwrap_err().contains("unexpected"));
        assert!(decode_bytes(b":abc\r\n").unwrap_err().contains("integer"));
        assert!(decode_bytes(b"$3\r\nab\r\n").is_err());
        assert!(decode_bytes(b"+OK\n").unwrap_err().contains("CRLF"));
    }

    #[test]
    fn error_replies_become_err_with_friendly_text() {
        assert!(Resp::Error("NOAUTH Authentication required.".into())
            .ok()
            .unwrap_err()
            .contains("password"));
        assert_eq!(
            Resp::Error("WRONGTYPE bad".into()).ok().unwrap_err(),
            "Redis error: WRONGTYPE bad"
        );
        assert!(Resp::Int(1).ok().is_ok());
    }

    #[test]
    fn parses_info_sections() {
        let info = parse_info(
            "# Server\r\nredis_version:7.2.4\r\nuptime_in_seconds:120\r\n\r\n# Memory\r\nused_memory_human:1.05M\r\n",
        );
        assert_eq!(info["redis_version"], "7.2.4");
        assert_eq!(info["uptime_in_seconds"], "120");
        assert_eq!(info["used_memory_human"], "1.05M");
        assert!(!info.contains_key("# Server"));
    }

    #[test]
    fn recognises_laravel_queue_keys_with_and_without_prefix() {
        assert_eq!(
            parse_queue_key("queues:default"),
            Some(("".into(), "default".into(), QueueRole::Pending))
        );
        assert_eq!(
            parse_queue_key("laravel_database_queues:emails:delayed"),
            Some(("laravel_database_".into(), "emails".into(), QueueRole::Delayed))
        );
        assert_eq!(
            parse_queue_key("queues:high:reserved"),
            Some(("".into(), "high".into(), QueueRole::Reserved))
        );
        assert_eq!(parse_queue_key("queues:default:notify"), None);
        assert_eq!(parse_queue_key("queues:"), None);
        assert_eq!(parse_queue_key("horizon:supervisors"), None);
    }

    #[test]
    fn groups_keys_into_one_row_per_queue() {
        let rows = group_queue_keys(&[
            "queues:default".into(),
            "queues:default:reserved".into(),
            "queues:default:notify".into(),
            "app_queues:mail:delayed".into(),
            "sessions:abc".into(),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].queue, "default");
        assert_eq!(rows[0].key, "queues:default");
        assert_eq!(rows[1].queue, "mail");
        assert_eq!(rows[1].key, "app_queues:mail");
    }

    #[test]
    fn summarises_a_laravel_job_payload() {
        let raw = r#"{"uuid":"u-1","displayName":"App\\Jobs\\SendMail","job":"Illuminate\\Queue\\CallQueuedHandler@call","attempts":2,"data":{"commandName":"App\\Jobs\\SendMail","command":"O:..."}}"#;
        let peek = summarise_job(raw);
        assert_eq!(peek.display_name.as_deref(), Some("App\\Jobs\\SendMail"));
        assert_eq!(peek.command_name.as_deref(), Some("App\\Jobs\\SendMail"));
        assert_eq!(peek.attempts, Some(2));
        assert_eq!(peek.id.as_deref(), Some("u-1"));
        assert!(peek.raw.contains("\n  \"uuid\""));
    }

    #[test]
    fn non_json_payloads_pass_through_verbatim() {
        let peek = summarise_job("not json");
        assert_eq!(peek.raw, "not json");
        assert_eq!(peek.display_name, None);
    }

    #[test]
    fn connection_refused_is_a_clear_error() {
        // Port 1 is never a Redis server.
        let err = redis_ping(RedisConn {
            host: Some("127.0.0.1".into()),
            port: Some(1),
            password: None,
            db: None,
        })
        .unwrap_err();
        assert!(err.contains("refused") || err.contains("connect"), "{err}");
    }
}
