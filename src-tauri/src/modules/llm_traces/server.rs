//! The collector itself: an HTTP/1.1 server on loopback that answers the
//! endpoints each SDK posts to, translates what arrives (see `langfuse`,
//! `langsmith`, `otel`) and stores it.
//!
//! | Path | Who sends it |
//! | --- | --- |
//! | `POST /api/public/otel/v1/traces` | Langfuse SDKs (Python v3+, JS v4+) |
//! | `POST /api/public/ingestion` | Langfuse v2 SDKs, LiteLLM and other integrations |
//! | `POST /api/public/scores` | Langfuse `score()` |
//! | `GET /info`, `POST /runs/multipart`, `POST /runs/batch`, `POST /runs`, `PATCH /runs/{id}`, `POST /feedback` | LangSmith SDKs, also under `/api/v1` |
//! | `POST /v1/traces`, `POST /otel/v1/traces` | any OpenTelemetry OTLP/HTTP exporter |
//!
//! Like Mailpit with SMTP credentials, it accepts any API key. It listens on
//! 127.0.0.1 and ::1, and, unless the user turns it off, on the host's address
//! on each Docker bridge, so apps in containers can post too (see
//! `platform`). Nothing it serves reads stored traces back: the page reads
//! them through Tauri commands, never over HTTP. A Langfuse public key is
//! still read, because it names the project.
//!
//! Without a key, who may write is decided by where a request comes from:
//!
//! - **Another machine.** A bridge address can be reached from the LAN (see
//!   `platform`), so a client there is answered only from inside one of the
//!   container networks or from this host (`Networks::admits`); anyone else
//!   gets a 403 before the body is read.
//! - **A web page.** Any site open in a browser can post to 127.0.0.1, and a
//!   form-style `text/plain` POST skips CORS preflight. Browsers mark their
//!   requests, and SDKs don't: every browser POST carries `Origin` (DNS
//!   rebinding included, since the page's origin is still the attacker's), and
//!   modern ones send `Sec-Fetch-Site` to localhost on every request. Either
//!   header gets a 403. (Node's `fetch` sends `Sec-Fetch-Mode`, but never
//!   these two.) `Host` isn't checked: a compose `extra_hosts` alias is a
//!   fine name for the collector, and the only requests a rebound page could
//!   still make are GETs of fixed answers.
//!
//! Bodies may be gzip-, deflate- or zstd-compressed: the Langfuse SDKs gzip
//! by default, and LangSmith's Python SDK zstds when `/info` says the server
//! takes it, which this one's does.

use super::model::{Batch, SOURCE_LANGFUSE, SOURCE_LANGSMITH, SOURCE_OTEL};
use super::platform::Networks;
use super::store::{IngestCounts, Store};
use super::{langfuse, langsmith, otel};
use base64::Engine;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderValue, CONTENT_ENCODING, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// OTLP recommends accepting 64 MiB; LangSmith batches stay under 20 MiB.
const MAX_BODY: usize = 64 * 1024 * 1024;
const RECENT_REQUESTS: usize = 30;

/// What the page shows under "Recent requests", so a misconfigured SDK — a
/// wrong path, a body that didn't parse — is visible rather than silent.
#[derive(Debug, Clone, Serialize)]
pub struct RequestLog {
    pub time_us: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    /// "langfuse", "langsmith", "otel", or null for something unrecognised.
    pub source: Option<&'static str>,
    pub note: String,
    /// The client's address: 127.0.0.1 for this machine, a bridge address
    /// (172.18.0.5) for a container.
    pub client: String,
}

pub struct Ctx {
    pub store: Arc<Store>,
    pub max_traces: AtomicU32,
    /// Told after every successful write; the app turns it into an event so
    /// an open page refreshes.
    pub on_ingest: Box<dyn Fn(IngestCounts) + Send + Sync>,
    recent: Mutex<VecDeque<RequestLog>>,
}

impl Ctx {
    pub fn new(store: Arc<Store>, max_traces: u32, on_ingest: Box<dyn Fn(IngestCounts) + Send + Sync>) -> Ctx {
        Ctx { store, max_traces: AtomicU32::new(max_traces), on_ingest, recent: Mutex::new(VecDeque::new()) }
    }

    pub fn recent(&self) -> Vec<RequestLog> {
        self.recent.lock().unwrap_or_else(PoisonError::into_inner).iter().rev().cloned().collect()
    }

    fn log(&self, entry: RequestLog) {
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if recent.len() == RECENT_REQUESTS {
            recent.pop_front();
        }
        recent.push_back(entry);
    }
}

/// Lists the extra addresses to listen on, the container bridges, and who
/// may use them: `platform::container_networks`, in the app.
pub type ExtraAddrs = fn() -> Networks;

/// The last scan's networks, shared by every bridge listener and refreshed by
/// each scan.
struct Bridges {
    scan: ExtraAddrs,
    known: Mutex<Networks>,
}

impl Bridges {
    fn new(scan: ExtraAddrs) -> Bridges {
        Bridges { scan, known: Mutex::new(Networks::default()) }
    }

    /// Reads the interfaces again and remembers what it found.
    async fn rescan(&self) -> Networks {
        let found = tokio::task::spawn_blocking(self.scan).await.unwrap_or_default();
        *self.known.lock().unwrap_or_else(PoisonError::into_inner) = found.clone();
        found
    }

    /// A container on any of the networks may use any bridge address: with
    /// `host-gateway`, `host.docker.internal` is `docker0`'s address even for
    /// a container on a compose network. A client the last scan doesn't place
    /// gets one fresh look, so a network `docker compose up` made a moment ago
    /// works before the next scheduled scan.
    async fn admit(&self, peer: IpAddr) -> bool {
        if self.known.lock().unwrap_or_else(PoisonError::into_inner).admits(peer) {
            return true;
        }
        self.rescan().await.admits(peer)
    }
}

/// How often to look for container networks created since the last look.
/// `docker compose up` makes a new bridge for each project.
const RESCAN_EVERY: Duration = Duration::from_secs(10);

pub struct ServerHandle {
    pub port: u16,
    shutdown: watch::Sender<bool>,
    extra: Arc<Mutex<Vec<IpAddr>>>,
}

impl ServerHandle {
    /// Stops accepting at once; requests in flight are allowed to finish.
    pub fn stop(self) {
        let _ = self.shutdown.send(true);
    }

    /// The extra addresses it is listening on now.
    pub fn extra_addrs(&self) -> Vec<IpAddr> {
        self.extra.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Binds 127.0.0.1 (required) and ::1 (when the machine has IPv6), so
/// `localhost` works whichever address a client's resolver tries first —
/// Node 18 tries ::1 and gives up there. With `extra`, also every address it
/// lists, now and every `RESCAN_EVERY`; one that can't be bound is skipped
/// and tried again on the next scan.
pub async fn start(port: u16, ctx: Arc<Ctx>, extra: Option<ExtraAddrs>) -> std::io::Result<ServerHandle> {
    let v4 = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = v4.local_addr()?.port();
    let v6 = TcpListener::bind(("::1", port)).await.ok();
    let (shutdown, watcher) = watch::channel(false);
    tokio::spawn(accept_loop(v4, ctx.clone(), watcher.clone(), None));
    if let Some(v6) = v6 {
        tokio::spawn(accept_loop(v6, ctx.clone(), watcher.clone(), None));
    }
    let bound = Arc::new(Mutex::new(Vec::new()));
    if let Some(list) = extra {
        let bridges = Arc::new(Bridges::new(list));
        bind_new(&bridges, port, &ctx, &watcher, &bound).await;
        let (ctx, bound, mut stop) = (ctx.clone(), bound.clone(), watcher.clone());
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    _ = tokio::time::sleep(RESCAN_EVERY) => bind_new(&bridges, port, &ctx, &stop, &bound).await,
                }
            }
        });
    }
    Ok(ServerHandle { port, shutdown, extra: bound })
}

async fn bind_new(
    bridges: &Arc<Bridges>,
    port: u16,
    ctx: &Arc<Ctx>,
    watcher: &watch::Receiver<bool>,
    bound: &Mutex<Vec<IpAddr>>,
) {
    for bridge in bridges.rescan().await.bridges {
        let addr = IpAddr::V4(bridge.addr);
        if bound.lock().unwrap_or_else(PoisonError::into_inner).contains(&addr) {
            continue;
        }
        if let Ok(listener) = TcpListener::bind((addr, port)).await {
            tokio::spawn(accept_loop(listener, ctx.clone(), watcher.clone(), Some(bridges.clone())));
            bound.lock().unwrap_or_else(PoisonError::into_inner).push(addr);
        }
    }
}

/// `bridges` is set for a listener on a bridge address, whose clients must
/// come from a container network; loopback listeners leave it unset.
async fn accept_loop(
    listener: TcpListener,
    ctx: Arc<Ctx>,
    mut shutdown: watch::Receiver<bool>,
    bridges: Option<Arc<Bridges>>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let ctx = ctx.clone();
                let bridges = bridges.clone();
                let mut shutdown = shutdown.clone();
                tokio::spawn(async move {
                    let peer = peer.ip().to_canonical();
                    let admitted = match &bridges {
                        Some(bridges) => bridges.admit(peer).await,
                        None => true,
                    };
                    let client = peer.to_string();
                    let service = service_fn(move |req| {
                        let ctx = ctx.clone();
                        let client = client.clone();
                        async move { Ok::<_, Infallible>(handle(ctx, req, client, admitted).await) }
                    });
                    let conn = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
                    tokio::pin!(conn);
                    tokio::select! {
                        _ = conn.as_mut() => {}
                        _ = shutdown.changed() => {
                            conn.as_mut().graceful_shutdown();
                            let _ = conn.await;
                        }
                    }
                });
            }
        }
    }
}

struct Reply {
    status: StatusCode,
    content_type: &'static str,
    body: Bytes,
    source: Option<&'static str>,
    note: String,
}

impl Reply {
    fn json(status: StatusCode, value: Value) -> Reply {
        Reply { status, content_type: "application/json", body: Bytes::from(value.to_string()), source: None, note: String::new() }
    }

    fn error(status: StatusCode, message: impl Into<String>) -> Reply {
        let message = message.into();
        let mut reply = Reply::json(status, json!({"message": message, "error": message}));
        reply.note = message;
        reply
    }

    fn from(mut self, source: &'static str) -> Reply {
        self.source = Some(source);
        self
    }

    fn noting(mut self, note: String) -> Reply {
        self.note = note;
        self
    }
}

/// `admitted` is false for a client on a bridge address from outside the
/// container networks (see the module comment).
async fn handle(ctx: Arc<Ctx>, req: Request<Incoming>, client: String, admitted: bool) -> Response<Full<Bytes>> {
    let method = req.method().clone();
    let path = normalize(req.uri().path());
    let headers = req.headers().clone();
    // Both refusals come before the body is read, so neither costs a download.
    let reply = if !admitted {
        Reply::error(StatusCode::FORBIDDEN, "only this machine and its containers can send traces here")
    } else if from_a_browser(&headers) {
        Reply::error(StatusCode::FORBIDDEN, "web pages can't send traces here; send them from the app's SDK")
    } else {
        match Limited::new(req.into_body(), MAX_BODY).collect().await {
            Err(_) => Reply::error(StatusCode::PAYLOAD_TOO_LARGE, format!("body larger than {} MB", MAX_BODY >> 20)),
            Ok(collected) => match decode_body(&headers, collected.to_bytes()) {
                Err(reply) => reply,
                Ok(body) => route(&ctx, &method, &path, &headers, body).await,
            },
        }
    };
    // Say how it arrived as well, so a compressed upload is visibly accepted.
    let encoding = headers.get(CONTENT_ENCODING).and_then(|v| v.to_str().ok()).filter(|e| !e.is_empty());
    let note = match encoding {
        Some(e) if reply.status.is_success() => format!("{} ({e})", reply.note),
        _ => reply.note,
    };
    ctx.log(RequestLog {
        time_us: super::model::now_us(),
        method: method.to_string(),
        path: path.clone(),
        status: reply.status.as_u16(),
        source: reply.source,
        note,
        client,
    });
    let mut response = Response::new(Full::new(reply.body));
    *response.status_mut() = reply.status;
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(reply.content_type));
    response
}

/// A request a web page made: see the module comment for why these two
/// headers, and not `Sec-Fetch-Mode`.
fn from_a_browser(headers: &HeaderMap) -> bool {
    headers.contains_key(hyper::header::ORIGIN) || headers.contains_key("sec-fetch-site")
}

/// LangSmith's API is mounted at `/api/v1` on a self-hosted install, and the
/// SDK appends `/runs/…` to whatever endpoint it is given, so both spellings
/// arrive. A trailing slash is ignored too.
fn normalize(path: &str) -> String {
    let path = path.trim_end_matches('/');
    let path = match path.strip_prefix("/api/v1") {
        Some(rest) if !rest.is_empty() => rest,
        _ => path,
    };
    if path.is_empty() { "/".to_string() } else { path.to_string() }
}

fn decode_body(headers: &HeaderMap, body: Bytes) -> Result<Bytes, Reply> {
    let encoding = headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let mut out = Vec::new();
    let read = match encoding.as_str() {
        "" | "identity" | "none" => return Ok(body),
        "gzip" | "x-gzip" => flate2::read::MultiGzDecoder::new(&body[..]).take(MAX_BODY as u64 + 1).read_to_end(&mut out),
        "deflate" => flate2::read::ZlibDecoder::new(&body[..]).take(MAX_BODY as u64 + 1).read_to_end(&mut out),
        "zstd" => match ruzstd::decoding::StreamingDecoder::new(&body[..]) {
            Ok(decoder) => decoder.take(MAX_BODY as u64 + 1).read_to_end(&mut out),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
        },
        other => return Err(Reply::error(StatusCode::UNSUPPORTED_MEDIA_TYPE, format!("unsupported Content-Encoding: {other}"))),
    };
    match read {
        Ok(_) if out.len() > MAX_BODY => Err(Reply::error(StatusCode::PAYLOAD_TOO_LARGE, "decompressed body too large")),
        Ok(_) => Ok(Bytes::from(out)),
        Err(e) => Err(Reply::error(StatusCode::BAD_REQUEST, format!("could not decompress the {encoding} body: {e}"))),
    }
}

fn content_type(headers: &HeaderMap) -> String {
    headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase()
}

/// The public key from `Authorization: Basic base64(pk:sk)`, or the header the
/// Langfuse SDKs send beside it.
fn langfuse_public_key(headers: &HeaderMap) -> Option<String> {
    let from_basic = headers
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic ").or_else(|| v.strip_prefix("basic ")))
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .and_then(|pair| pair.split(':').next().map(str::to_string))
        .filter(|pk| !pk.is_empty());
    from_basic.or_else(|| {
        headers.get("x-langfuse-public-key").and_then(|v| v.to_str().ok()).map(str::to_string).filter(|s| !s.is_empty())
    })
}

fn parse_json(body: &[u8]) -> Result<Value, Reply> {
    serde_json::from_slice(body).map_err(|e| Reply::error(StatusCode::BAD_REQUEST, format!("body is not JSON: {e}")))
}

/// Parsing and translating a batch is CPU work that grows with the body, up
/// to `MAX_BODY`, so the batch routes do it on the blocking pool rather than
/// hold a runtime worker that other connections are waiting on.
async fn off_runtime<T: Send + 'static>(work: impl FnOnce() -> Result<T, Reply> + Send + 'static) -> Result<T, Reply> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|e| Err(Reply::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))
}

/// Writes on a blocking thread — SQLite is synchronous — and reports what
/// was stored in the request log line.
async fn store_batch(ctx: &Arc<Ctx>, batch: Batch, source: &'static str) -> Result<String, Reply> {
    if batch.is_empty() {
        return Ok("nothing to store".into());
    }
    let ctx2 = ctx.clone();
    let max = ctx.max_traces.load(Ordering::Relaxed);
    let counts = tokio::task::spawn_blocking(move || ctx2.store.ingest(&batch, source, max))
        .await
        .map_err(|e| Reply::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| Reply::error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    (ctx.on_ingest)(counts);
    let mut parts = Vec::new();
    if counts.observations > 0 {
        parts.push(format!("{} observation{}", counts.observations, if counts.observations == 1 { "" } else { "s" }));
    }
    if counts.scores > 0 {
        parts.push(format!("{} score{}", counts.scores, if counts.scores == 1 { "" } else { "s" }));
    }
    if parts.is_empty() && (counts.traces > 0 || counts.dropped == 0) {
        parts.push(format!("{} trace update{}", counts.traces, if counts.traces == 1 { "" } else { "s" }));
    }
    if counts.dropped > 0 {
        let s = if counts.dropped == 1 { "" } else { "s" };
        parts.push(format!("{} observation{s} dropped: the trace is full", counts.dropped));
    }
    Ok(parts.join(", "))
}

macro_rules! tryr {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(reply) => return reply,
        }
    };
}

async fn route(ctx: &Arc<Ctx>, method: &Method, path: &str, headers: &HeaderMap, body: Bytes) -> Reply {
    match (method, path) {
        (&Method::GET, "/") => Reply {
            status: StatusCode::OK,
            content_type: "text/plain; charset=utf-8",
            body: Bytes::from_static(
                b"DevsPro LLM trace collector. Langfuse: /api/public/... LangSmith: /runs/... OTLP: /v1/traces\n",
            ),
            source: None,
            note: String::new(),
        },

        // ---- Langfuse ----
        (&Method::GET, "/api/public/health") => {
            Reply::json(StatusCode::OK, json!({"status": "OK", "version": "devspro-local"})).from(SOURCE_LANGFUSE)
        }
        (&Method::GET, "/api/public/projects") => {
            let project = langfuse_public_key(headers).unwrap_or_else(|| "devspro-local".into());
            Reply::json(
                StatusCode::OK,
                json!({"data": [{
                    "id": project, "name": "DevsPro (local)",
                    "organization": {"id": "devspro", "name": "DevsPro"}, "metadata": {}
                }]}),
            )
            .from(SOURCE_LANGFUSE)
        }
        (&Method::POST, "/api/public/ingestion") => {
            let project = langfuse_public_key(headers);
            let ingested = tryr!(off_runtime(move || Ok(langfuse::ingest(&parse_json(&body)?, project.as_deref())))
                .await
                .map_err(|r| r.from(SOURCE_LANGFUSE)));
            let errors = ingested.response["errors"].as_array().map_or(0, Vec::len);
            let note = tryr!(store_batch(ctx, ingested.batch, SOURCE_LANGFUSE).await.map_err(|r| r.from(SOURCE_LANGFUSE)));
            let note = if errors > 0 { format!("{note}; {errors} event(s) rejected") } else { note };
            Reply::json(StatusCode::MULTI_STATUS, ingested.response).from(SOURCE_LANGFUSE).noting(note)
        }
        (&Method::POST, "/api/public/scores") => {
            let value = tryr!(parse_json(&body).map_err(|r| r.from(SOURCE_LANGFUSE)));
            let scores = tryr!(langfuse::score_request(&value)
                .map_err(|e| Reply::error(StatusCode::BAD_REQUEST, e).from(SOURCE_LANGFUSE)));
            let single_id = (!value.is_array()).then(|| scores.first().map(|s| s.id.clone())).flatten();
            let batch = Batch { scores, ..Default::default() };
            let note = tryr!(store_batch(ctx, batch, SOURCE_LANGFUSE).await.map_err(|r| r.from(SOURCE_LANGFUSE)));
            match single_id {
                Some(id) => Reply::json(StatusCode::OK, json!({"id": id})),
                None => Reply::json(StatusCode::ACCEPTED, json!({"message": "Accepted"})),
            }
            .from(SOURCE_LANGFUSE)
            .noting(note)
        }
        (&Method::POST, "/api/public/otel/v1/traces") => {
            let project = langfuse_public_key(headers);
            otlp(ctx, headers, body, otel::Flavor::Langfuse, project).await.from(SOURCE_LANGFUSE)
        }

        // ---- OpenTelemetry ----
        (&Method::POST, "/v1/traces") | (&Method::POST, "/otel/v1/traces") => {
            otlp(ctx, headers, body, otel::Flavor::Generic, None).await.from(SOURCE_OTEL)
        }

        // ---- LangSmith ----
        (&Method::GET, "/info") => Reply::json(StatusCode::OK, langsmith::info()).from(SOURCE_LANGSMITH),
        (&Method::POST, "/runs") => {
            let value = tryr!(parse_json(&body).map_err(|r| r.from(SOURCE_LANGSMITH)));
            let batch = tryr!(langsmith::batch_to_batch(std::slice::from_ref(&value), &[])
                .map_err(|e| Reply::error(StatusCode::UNPROCESSABLE_ENTITY, e).from(SOURCE_LANGSMITH)));
            accepted(ctx, batch).await
        }
        (&Method::POST, "/runs/batch") => {
            let batch = tryr!(off_runtime(move || {
                let value = parse_json(&body)?;
                let list = |key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
                langsmith::batch_to_batch(&list("post"), &list("patch"))
                    .map_err(|e| Reply::error(StatusCode::UNPROCESSABLE_ENTITY, e))
            })
            .await
            .map_err(|r| r.from(SOURCE_LANGSMITH)));
            accepted(ctx, batch).await
        }
        (&Method::POST, "/runs/multipart") => {
            // Not `content_type()`: that lowercases, and a boundary is case-sensitive.
            let raw_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
            let Some(boundary) = langsmith::boundary(raw_type) else {
                return Reply::error(StatusCode::BAD_REQUEST, "multipart body without a boundary").from(SOURCE_LANGSMITH);
            };
            let batch = tryr!(off_runtime(move || {
                langsmith::parse_multipart(&body, &boundary)
                    .and_then(|parts| langsmith::multipart_to_batch(&parts))
                    .map_err(|e| Reply::error(StatusCode::UNPROCESSABLE_ENTITY, e))
            })
            .await
            .map_err(|r| r.from(SOURCE_LANGSMITH)));
            accepted(ctx, batch).await
        }
        (&Method::PATCH, p) if p.starts_with("/runs/") => {
            let id = &p["/runs/".len()..];
            let value = tryr!(parse_json(&body).map_err(|r| r.from(SOURCE_LANGSMITH)));
            let batch = tryr!(langsmith::patch_to_batch(id, &value)
                .map_err(|e| Reply::error(StatusCode::UNPROCESSABLE_ENTITY, e).from(SOURCE_LANGSMITH)));
            accepted(ctx, batch).await
        }
        (&Method::POST, "/feedback") => {
            let value = tryr!(parse_json(&body).map_err(|r| r.from(SOURCE_LANGSMITH)));
            let score = tryr!(langsmith::feedback_to_score(&value)
                .map_err(|e| Reply::error(StatusCode::UNPROCESSABLE_ENTITY, e).from(SOURCE_LANGSMITH)));
            // create_feedback() parses the reply as a Feedback, so echo one.
            let now = chrono::Utc::now().to_rfc3339();
            let mut echo = value.as_object().cloned().unwrap_or_default();
            echo.insert("id".into(), Value::String(score.id.clone()));
            echo.entry("created_at").or_insert(Value::String(now.clone()));
            echo.insert("modified_at".into(), Value::String(now));
            let note = tryr!(store_batch(ctx, Batch { scores: vec![score], ..Default::default() }, SOURCE_LANGSMITH)
                .await
                .map_err(|r| r.from(SOURCE_LANGSMITH)));
            Reply::json(StatusCode::OK, Value::Object(echo)).from(SOURCE_LANGSMITH).noting(note)
        }

        _ => Reply::error(
            StatusCode::NOT_FOUND,
            format!("{method} {path} isn't something DevsPro's trace collector handles"),
        ),
    }
}

async fn accepted(ctx: &Arc<Ctx>, batch: Batch) -> Reply {
    match store_batch(ctx, batch, SOURCE_LANGSMITH).await {
        Ok(note) => Reply::json(StatusCode::ACCEPTED, json!({})).from(SOURCE_LANGSMITH).noting(note),
        Err(reply) => reply.from(SOURCE_LANGSMITH),
    }
}

/// OTLP answers in the encoding it was asked in: an empty
/// `ExportTraceServiceResponse` is zero bytes of protobuf, or `{}`.
async fn otlp(ctx: &Arc<Ctx>, headers: &HeaderMap, body: Bytes, flavor: otel::Flavor, project: Option<String>) -> Reply {
    let json = content_type(headers).contains("json");
    let batch = tryr!(off_runtime(move || {
        let spans = if json { otel::decode_json(&parse_json(&body)?) } else { otel::decode_protobuf(&body) };
        let spans = spans.map_err(|e| Reply::error(StatusCode::BAD_REQUEST, e))?;
        Ok(otel::to_batch(spans, flavor, project.as_deref()))
    })
    .await);
    let source = match flavor {
        otel::Flavor::Langfuse => SOURCE_LANGFUSE,
        otel::Flavor::Generic => SOURCE_OTEL,
    };
    let note = tryr!(store_batch(ctx, batch, source).await);
    let reply = if json {
        Reply::json(StatusCode::OK, json!({}))
    } else {
        Reply { status: StatusCode::OK, content_type: "application/x-protobuf", body: Bytes::new(), source: None, note: String::new() }
    };
    reply.noting(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    async fn running() -> (ServerHandle, Arc<Ctx>, String) {
        let ctx = Arc::new(Ctx::new(Arc::new(Store::open_in_memory()), 1000, Box::new(|_| {})));
        let handle = start(0, ctx.clone(), None).await.unwrap();
        let base = format!("http://127.0.0.1:{}", handle.port);
        (handle, ctx, base)
    }

    #[test]
    fn compressed_bodies_are_unpacked() {
        let plain = b"{\"hello\":\"world\"}".to_vec();
        let zstd = ruzstd::encoding::compress_to_vec(&plain[..], ruzstd::encoding::CompressionLevel::Fastest);
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("zstd"));
        assert_eq!(decode_body(&headers, Bytes::from(zstd)).ok().unwrap(), Bytes::from(plain.clone()));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
        assert!(decode_body(&headers, Bytes::from(plain)).is_err());
    }

    #[test]
    fn paths_are_normalised() {
        assert_eq!(normalize("/api/v1/runs/multipart"), "/runs/multipart");
        assert_eq!(normalize("/api/v1/info/"), "/info");
        assert_eq!(normalize("/api/public/ingestion/"), "/api/public/ingestion");
        assert_eq!(normalize("/api/v1"), "/api/v1");
        assert_eq!(normalize("/"), "/");
    }

    #[tokio::test]
    async fn langfuse_langsmith_and_otlp_requests_all_land_in_the_store() {
        let (handle, ctx, base) = running().await;
        let client = reqwest::Client::new();

        // Langfuse legacy batch, with Basic auth naming the project.
        let r = client
            .post(format!("{base}/api/public/ingestion"))
            .basic_auth("pk-lf-test", Some("sk-lf-test"))
            .header("content-type", "application/json")
            .body(json!({"batch": [{"id": "e1", "type": "trace-create", "timestamp": "2024-05-01T10:00:00Z",
                "body": {"id": "lf-trace", "name": "from-langfuse"}}]}).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 207);

        // LangSmith /info, then a gzip-free JSON batch under the /api/v1 prefix.
        let info: Value =
            serde_json::from_str(&client.get(format!("{base}/api/v1/info")).send().await.unwrap().text().await.unwrap())
                .unwrap();
        assert_eq!(info["batch_ingest_config"]["use_multipart_endpoint"], true);
        let r = client
            .post(format!("{base}/runs/batch"))
            .header("content-type", "application/json")
            .body(json!({"post": [{"id": "ls-root", "name": "from-langsmith", "run_type": "chain",
                "start_time": "2024-05-01T10:00:00Z", "session_name": "proj"}], "patch": []}).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 202);

        // OTLP JSON, gzipped like the Langfuse SDKs send it.
        let otlp = json!({"resourceSpans": [{"scopeSpans": [{"spans": [{
            "traceId": "0102030405060708090a0b0c0d0e0f10", "spanId": "0102030405060708", "name": "from-otel",
            "startTimeUnixNano": "1714557600000000000", "endTimeUnixNano": "1714557601000000000"}]}]}]});
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(otlp.to_string().as_bytes()).unwrap();
        let r = client
            .post(format!("{base}/v1/traces"))
            .header("content-type", "application/json")
            .header("content-encoding", "gzip")
            .body(gz.finish().unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.text().await.unwrap(), "{}");

        // Something no SDK sends is refused, and logged.
        let r = client.get(format!("{base}/api/public/v2/prompts/x")).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 404);

        let list = ctx.store.list(&Default::default()).unwrap();
        let mut names: Vec<_> = list.traces.iter().filter_map(|t| t.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["from-langfuse", "from-langsmith", "from-otel"]);
        let langfuse = list.traces.iter().find(|t| t.id == "lf-trace").unwrap();
        assert_eq!(langfuse.project.as_deref(), Some("pk-lf-test"));

        let recent = ctx.recent();
        assert_eq!(recent[0].status, 404);
        assert_eq!(recent.iter().filter(|r| r.status < 300).count(), 4);
        handle.stop();
    }

    /// Not a test: the collector on its own, for checking real SDKs against
    /// it without the app. `scripts/llm-traces-sdk-check/run.sh` drives it:
    ///
    /// ```sh
    /// LLM_TRACES_PORT=4399 LLM_TRACES_DB=/tmp/traces.db LLM_TRACES_SECS=300 \
    ///   cargo test --lib llm_traces::server::tests::serve -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn serve() {
        let var = |name: &str| std::env::var(name).ok();
        let port = var("LLM_TRACES_PORT").and_then(|p| p.parse().ok()).unwrap_or(4399);
        let secs = var("LLM_TRACES_SECS").and_then(|s| s.parse().ok()).unwrap_or(300);
        let db = var("LLM_TRACES_DB").unwrap_or_else(|| std::env::temp_dir().join("llm-traces-check.db").display().to_string());
        let store = Arc::new(Store::open(std::path::Path::new(&db)).unwrap());
        let ctx = Arc::new(Ctx::new(store, 10_000, Box::new(|counts| println!("stored {counts:?}"))));
        let handle = start(port, ctx.clone(), Some(super::super::platform::container_networks)).await.unwrap();
        println!("also listening on {:?}", handle.extra_addrs());
        println!("collector listening on http://127.0.0.1:{} (db {db}) for {secs}s", handle.port);
        let stop_file = format!("{db}.stop");
        for _ in 0..secs * 10 {
            if std::path::Path::new(&stop_file).exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        for r in ctx.recent().iter().rev() {
            println!("{} {} -> {} [{}] {} (from {})", r.method, r.path, r.status, r.source.unwrap_or("-"), r.note, r.client);
        }
        handle.stop();
    }

    /// 127.0.0.2/30 stands in for a Docker bridge: on Linux all of 127/8 is
    /// loopback, so it can be bound, and connected from, without touching the
    /// network. 127.0.0.3 is a container on it, 127.0.0.7 one of the host's
    /// own addresses, and 127.0.0.9 a machine on the LAN that routed its way
    /// to the bridge address.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn bridge_addresses_answer_containers_and_the_host_and_refuse_everyone_else() {
        fn bridge() -> Networks {
            Networks {
                bridges: vec![super::super::platform::Bridge { addr: std::net::Ipv4Addr::new(127, 0, 0, 2), prefix: 30 }],
                own: vec![IpAddr::from([127, 0, 0, 7])],
            }
        }
        let ctx = Arc::new(Ctx::new(Arc::new(Store::open_in_memory()), 1000, Box::new(|_| {})));
        let handle = start(0, ctx.clone(), Some(bridge)).await.unwrap();
        assert_eq!(handle.extra_addrs(), vec![IpAddr::from([127, 0, 0, 2])]);
        let otlp = json!({"resourceSpans": [{"scopeSpans": [{"spans": [{
            "traceId": "0102030405060708090a0b0c0d0e0f10", "spanId": "0102030405060708", "name": "from-a-container",
            "startTimeUnixNano": "1714557600000000000"}]}]}]})
        .to_string();
        let post_from = |from: [u8; 4]| {
            let client = reqwest::Client::builder().local_address(IpAddr::from(from)).build().unwrap();
            client
                .post(format!("http://127.0.0.2:{}/v1/traces", handle.port))
                .header("content-type", "application/json")
                .body(otlp.clone())
                .send()
        };

        assert_eq!(post_from([127, 0, 0, 3]).await.unwrap().status().as_u16(), 200);
        assert_eq!(ctx.recent()[0].client, "127.0.0.3");
        assert_eq!(post_from([127, 0, 0, 7]).await.unwrap().status().as_u16(), 200);
        assert_eq!(ctx.recent()[0].client, "127.0.0.7");

        assert_eq!(post_from([127, 0, 0, 9]).await.unwrap().status().as_u16(), 403);
        assert_eq!(ctx.recent()[0].client, "127.0.0.9");
        assert!(ctx.recent()[0].note.contains("only this machine and its containers"));
        // The two admitted posts carried the same span.
        assert_eq!(ctx.store.stats().unwrap().observations, 1);
        handle.stop();
    }

    /// What a page's `fetch` or form post to the collector looks like, next to
    /// Node's `fetch`, which also sends `Sec-Fetch-Mode` and must still work.
    #[tokio::test]
    async fn requests_from_web_pages_are_refused() {
        let (handle, ctx, base) = running().await;
        let client = reqwest::Client::new();
        let body = json!({"batch": [{"id": "e1", "type": "trace-create", "timestamp": "2024-05-01T10:00:00Z",
            "body": {"id": "from-a-page"}}]})
        .to_string();
        let post = |extra: &[(&'static str, &'static str)]| {
            let mut request = client
                .post(format!("{base}/api/public/ingestion"))
                .header("content-type", "text/plain")
                .body(body.clone());
            for (name, value) in extra {
                request = request.header(*name, *value);
            }
            request.send()
        };

        let r = post(&[("origin", "https://evil.example")]).await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
        let r = post(&[("origin", "null")]).await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
        let r = post(&[("sec-fetch-site", "cross-site"), ("sec-fetch-mode", "no-cors")]).await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
        assert!(ctx.recent()[0].note.contains("web pages can't send traces"));
        assert_eq!(ctx.store.stats().unwrap().traces, 0);

        let r = post(&[("sec-fetch-mode", "cors"), ("user-agent", "node")]).await.unwrap();
        assert_eq!(r.status().as_u16(), 207);
        assert_eq!(ctx.store.stats().unwrap().traces, 1);
        handle.stop();
    }

    #[tokio::test]
    async fn the_port_is_released_on_stop() {
        let (handle, _ctx, _base) = running().await;
        let port = handle.port;
        handle.stop();
        // The accept loop exits on the next poll; give it a moment.
        for _ in 0..50 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("port {port} still bound after stop");
    }
}
