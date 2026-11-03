//! Where caught traces live: a SQLite file of the module's own,
//! `<app data>/llm-traces/traces.db`, not the shared `app.db`.
//!
//! Two reasons for the separate file. An instrumented app can post spans many
//! times a second, and every other module shares `app.db`'s single connection;
//! traffic here must not queue behind (or hold up) a sites list. And this data
//! is disposable by nature — "Clear all" or deleting the file loses nothing
//! anyone configured — so it shouldn't sit in the frozen, append-only
//! migration list `db::later_migrations` keeps for the app's real state.
//!
//! Writes are upserts that merge (see `model`): a field the incoming row leaves
//! out keeps its stored value, metadata objects merge key by key, and a trace's
//! start and end widen to cover every observation that arrives for it.

use super::model::{now_us, Batch, ObservationUpsert, ScoreUpsert, TraceUpsert};
use rusqlite::{named_params, params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// One stored field larger than this is cut down to a marked excerpt. A base64
/// image in a prompt can run to megabytes, and every trace detail ships its
/// fields to the webview in one piece.
const MAX_FIELD_BYTES: usize = 512 * 1024;
const EXCERPT_BYTES: usize = 32 * 1024;
const PREVIEW_CHARS: usize = 160;

/// `max_traces` alone doesn't bound the file: one trace id can collect
/// observations without end, from a loop in an app or anything else that can
/// reach the collector. Past this a trace takes no new observations (updates
/// to ones it has still land). A long agent run is a few thousand, and the
/// trace view draws every one.
const MAX_OBSERVATIONS_PER_TRACE: i64 = 10_000;
/// Past this much data the oldest traces go, whatever `max_traces` says.
/// Typical traces are a few kB each, so retention by count is what normally
/// applies; this is the backstop for traces full of large prompts.
const MAX_STORE_BYTES: u64 = 2 << 30;
/// Scores still waiting for their observation, newest kept (see `upsert_score`).
const MAX_WAITING_SCORES: u32 = 10_000;

/// This file's own schema history. Append only, as with `db::later_migrations`.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE traces (
        id TEXT PRIMARY KEY,
        source TEXT NOT NULL,
        project TEXT,
        name TEXT,
        user_id TEXT,
        session_id TEXT,
        tags TEXT,
        input TEXT,
        output TEXT,
        -- One line of each, worked out on write from the whole value, so the
        -- list never has to load or parse a payload.
        input_preview TEXT,
        output_preview TEXT,
        metadata TEXT,
        release TEXT,
        version TEXT,
        environment TEXT,
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        received_us INTEGER NOT NULL,
        updated_us INTEGER NOT NULL
    );
    CREATE INDEX traces_start ON traces(start_us DESC);
    CREATE TABLE observations (
        id TEXT PRIMARY KEY,
        trace_id TEXT NOT NULL,
        parent_id TEXT,
        kind TEXT,
        name TEXT,
        start_us INTEGER,
        end_us INTEGER,
        completion_start_us INTEGER,
        model TEXT,
        model_parameters TEXT,
        input TEXT,
        output TEXT,
        metadata TEXT,
        input_tokens INTEGER,
        output_tokens INTEGER,
        total_tokens INTEGER,
        cost REAL,
        level TEXT,
        status_message TEXT,
        attributes TEXT,
        received_us INTEGER NOT NULL
    );
    CREATE INDEX observations_trace ON observations(trace_id);
    CREATE TABLE scores (
        id TEXT PRIMARY KEY,
        trace_id TEXT,
        observation_id TEXT,
        name TEXT NOT NULL,
        value REAL,
        string_value TEXT,
        comment TEXT,
        time_us INTEGER NOT NULL
    );
    CREATE INDEX scores_trace ON scores(trace_id);
"];

pub struct Store {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct IngestCounts {
    pub traces: usize,
    pub observations: usize,
    pub scores: usize,
    /// New observations refused because their trace was full.
    pub dropped: usize,
}

/// What `ingest` trims the store to: the app's settings and the constants
/// above, or small numbers in tests.
#[derive(Debug, Clone, Copy)]
struct Limits {
    traces: u32,
    observations_per_trace: i64,
    bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListQuery {
    pub search: Option<String>,
    pub source: Option<String>,
    pub project: Option<String>,
    pub errors_only: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceSummary {
    pub id: String,
    pub source: String,
    pub project: Option<String>,
    pub name: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub tags: Vec<String>,
    pub start_us: i64,
    pub end_us: Option<i64>,
    pub updated_us: i64,
    pub observation_count: i64,
    pub generation_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost: Option<f64>,
    pub error_count: i64,
    pub models: Vec<String>,
    pub input_preview: Option<String>,
    pub output_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceList {
    pub traces: Vec<TraceSummary>,
    /// Matching the filters, not just this page.
    pub total: i64,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservationRow {
    pub id: String,
    pub trace_id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub name: Option<String>,
    pub start_us: Option<i64>,
    pub end_us: Option<i64>,
    pub completion_start_us: Option<i64>,
    pub model: Option<String>,
    pub model_parameters: Option<Value>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub metadata: Option<Value>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub level: Option<String>,
    pub status_message: Option<String>,
    pub attributes: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreRow {
    pub id: String,
    pub observation_id: Option<String>,
    pub name: String,
    pub value: Option<f64>,
    pub string_value: Option<String>,
    pub comment: Option<String>,
    pub time_us: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceDetail {
    pub summary: TraceSummary,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub metadata: Option<Value>,
    pub release: Option<String>,
    pub version: Option<String>,
    pub environment: Option<String>,
    pub observations: Vec<ObservationRow>,
    pub scores: Vec<ScoreRow>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StoreStats {
    pub traces: i64,
    pub observations: i64,
    pub bytes: u64,
}

fn err(e: rusqlite::Error) -> String {
    format!("trace store: {e}")
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(err)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(err)?;
        migrate(&conn)?;
        Ok(Store { conn: Mutex::new(conn), path: Some(path.to_path_buf()) })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        Store { conn: Mutex::new(conn), path: None }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Applies one request's upserts in a single transaction, then trims the
    /// store back to `max_traces` and `MAX_STORE_BYTES`, oldest first.
    pub fn ingest(&self, batch: &Batch, implicit_source: &'static str, max_traces: u32) -> Result<IngestCounts, String> {
        let limits = Limits {
            traces: max_traces,
            observations_per_trace: MAX_OBSERVATIONS_PER_TRACE,
            bytes: MAX_STORE_BYTES,
        };
        self.ingest_within(batch, implicit_source, limits)
    }

    fn ingest_within(&self, batch: &Batch, implicit_source: &'static str, limits: Limits) -> Result<IngestCounts, String> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(err)?;
        let now = now_us();
        let mut counts = IngestCounts::default();
        for trace in &batch.traces {
            upsert_trace(&tx, trace, now)?;
            counts.traces += 1;
        }
        // Each trace's observation count, read once per request and kept up
        // to date as this request adds to it.
        let mut held: HashMap<String, i64> = HashMap::new();
        for obs in &batch.observations {
            let trace_id = resolve_trace_id(&tx, obs)?;
            if !observation_exists(&tx, &obs.id)? {
                let count = match held.entry(trace_id.clone()) {
                    Entry::Occupied(held) => held.into_mut(),
                    Entry::Vacant(slot) => slot.insert(
                        tx.query_row("SELECT COUNT(*) FROM observations WHERE trace_id = ?1", [&trace_id], |r| r.get(0))
                            .map_err(err)?,
                    ),
                };
                if *count >= limits.observations_per_trace {
                    counts.dropped += 1;
                    continue;
                }
                *count += 1;
            }
            upsert_observation(&tx, obs, &trace_id, implicit_source, now)?;
            counts.observations += 1;
        }
        for score in &batch.scores {
            if upsert_score(&tx, score, implicit_source, now)? {
                counts.scores += 1;
            }
        }
        prune(&tx, limits)?;
        tx.commit().map_err(err)?;
        Ok(counts)
    }

    pub fn list(&self, query: &ListQuery) -> Result<TraceList, String> {
        let conn = self.lock();
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(source) = query.source.as_deref().filter(|s| !s.is_empty()) {
            clauses.push("t.source = ?".into());
            args.push(source.to_string().into());
        }
        if let Some(project) = query.project.as_deref().filter(|s| !s.is_empty()) {
            clauses.push("t.project = ?".into());
            args.push(project.to_string().into());
        }
        if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let like = format!("%{}%", search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            let matches = |columns: &[&str]| {
                columns.iter().map(|c| format!("{c} LIKE ? ESCAPE '\\'")).collect::<Vec<_>>().join(" OR ")
            };
            let trace_columns = ["t.id", "t.name", "t.user_id", "t.session_id", "t.tags", "t.input", "t.output"];
            let observation_columns = ["o.name", "o.model"];
            clauses.push(format!(
                "({} OR EXISTS (SELECT 1 FROM observations o WHERE o.trace_id = t.id AND ({})))",
                matches(&trace_columns),
                matches(&observation_columns)
            ));
            for _ in 0..trace_columns.len() + observation_columns.len() {
                args.push(like.clone().into());
            }
        }
        if query.errors_only {
            clauses.push("EXISTS (SELECT 1 FROM observations o WHERE o.trace_id = t.id AND o.level = 'ERROR')".into());
        }
        let filter = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };

        let total: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM traces t {filter}"), params_from_iter(args.iter()), |r| r.get(0))
            .map_err(err)?;

        let limit = query.limit.unwrap_or(100).clamp(1, 500);
        let offset = query.offset.unwrap_or(0);
        let sql = format!(
            "SELECT t.id, t.source, t.project, t.name, t.user_id, t.session_id, t.tags, t.start_us, t.end_us, \
             t.updated_us, t.input_preview, t.output_preview \
             FROM traces t {filter} ORDER BY t.start_us DESC LIMIT {limit} OFFSET {offset}"
        );
        let mut stmt = conn.prepare(&sql).map_err(err)?;
        let mut traces: Vec<TraceSummary> = stmt
            .query_map(params_from_iter(args.iter()), summary_from_row)
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        drop(stmt);
        fill_aggregates(&conn, &mut traces)?;

        let mut stmt = conn
            .prepare("SELECT DISTINCT project FROM traces WHERE project IS NOT NULL ORDER BY project LIMIT 200")
            .map_err(err)?;
        let projects = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(TraceList { traces, total, projects })
    }

    pub fn get(&self, id: &str) -> Result<Option<TraceDetail>, String> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT t.id, t.source, t.project, t.name, t.user_id, t.session_id, t.tags, t.start_us, t.end_us, \
                 t.updated_us, t.input_preview, t.output_preview, \
                 t.input, t.output, t.metadata, t.release, t.version, t.environment FROM traces t WHERE t.id = ?1",
                [id],
                |r| {
                    Ok((
                        summary_from_row(r)?,
                        json_col(r, 12)?,
                        json_col(r, 13)?,
                        json_col(r, 14)?,
                        r.get::<_, Option<String>>(15)?,
                        r.get::<_, Option<String>>(16)?,
                        r.get::<_, Option<String>>(17)?,
                    ))
                },
            )
            .optional()
            .map_err(err)?;
        let Some((summary, input, output, metadata, release, version, environment)) = row else {
            return Ok(None);
        };
        let mut summaries = vec![summary];
        fill_aggregates(&conn, &mut summaries)?;
        let summary = summaries.pop().expect("one summary in, one out");

        let mut stmt = conn
            .prepare(
                "SELECT id, trace_id, parent_id, kind, name, start_us, end_us, completion_start_us, model, \
                 model_parameters, input, output, metadata, input_tokens, output_tokens, total_tokens, cost, level, \
                 status_message, attributes FROM observations WHERE trace_id = ?1 \
                 ORDER BY COALESCE(start_us, received_us), received_us, id",
            )
            .map_err(err)?;
        let observations = stmt
            .query_map([id], |r| {
                Ok(ObservationRow {
                    id: r.get(0)?,
                    trace_id: r.get(1)?,
                    parent_id: r.get(2)?,
                    kind: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "span".to_string()),
                    name: r.get(4)?,
                    start_us: r.get(5)?,
                    end_us: r.get(6)?,
                    completion_start_us: r.get(7)?,
                    model: r.get(8)?,
                    model_parameters: json_col(r, 9)?,
                    input: json_col(r, 10)?,
                    output: json_col(r, 11)?,
                    metadata: json_col(r, 12)?,
                    input_tokens: r.get(13)?,
                    output_tokens: r.get(14)?,
                    total_tokens: r.get(15)?,
                    cost: r.get(16)?,
                    level: r.get(17)?,
                    status_message: r.get(18)?,
                    attributes: json_col(r, 19)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        drop(stmt);

        let mut stmt = conn
            .prepare(
                "SELECT id, observation_id, name, value, string_value, comment, time_us FROM scores \
                 WHERE trace_id = ?1 ORDER BY time_us",
            )
            .map_err(err)?;
        let scores = stmt
            .query_map([id], |r| {
                Ok(ScoreRow {
                    id: r.get(0)?,
                    observation_id: r.get(1)?,
                    name: r.get(2)?,
                    value: r.get(3)?,
                    string_value: r.get(4)?,
                    comment: r.get(5)?,
                    time_us: r.get(6)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;

        Ok(Some(TraceDetail { summary, input, output, metadata, release, version, environment, observations, scores }))
    }

    pub fn delete(&self, ids: &[String]) -> Result<usize, String> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(err)?;
        let mut deleted = 0;
        for id in ids {
            deleted += tx.execute("DELETE FROM traces WHERE id = ?1", [id]).map_err(err)?;
            tx.execute("DELETE FROM observations WHERE trace_id = ?1", [id]).map_err(err)?;
            tx.execute("DELETE FROM scores WHERE trace_id = ?1", [id]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(deleted)
    }

    pub fn clear(&self) -> Result<(), String> {
        let conn = self.lock();
        conn.execute_batch("DELETE FROM scores; DELETE FROM observations; DELETE FROM traces;").map_err(err)?;
        // Give the space back: a cleared store should be small on disk too.
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        Ok(())
    }

    pub fn stats(&self) -> Result<StoreStats, String> {
        let conn = self.lock();
        let traces = conn.query_row("SELECT COUNT(*) FROM traces", [], |r| r.get(0)).map_err(err)?;
        let observations = conn.query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0)).map_err(err)?;
        let bytes = self
            .path
            .as_ref()
            .map(|p| {
                ["", "-wal"]
                    .iter()
                    .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", p.display())).ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0);
        Ok(StoreStats { traces, observations, bytes })
    }
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version > current {
            conn.execute_batch(sql).map_err(err)?;
            conn.pragma_update(None, "user_version", version).map_err(err)?;
        }
    }
    Ok(())
}

/// JSON text for a column, cut down to a marked excerpt when it is too large.
fn to_json_text(value: &Option<Value>) -> Option<String> {
    let value = value.as_ref()?;
    let text = value.to_string();
    if text.len() <= MAX_FIELD_BYTES {
        return Some(text);
    }
    let mut cut = EXCERPT_BYTES;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let excerpt = format!(
        "{}\n\n[DevsPro kept the first {} KB of {} KB]",
        &text[..cut],
        EXCERPT_BYTES / 1024,
        text.len() / 1024
    );
    Some(Value::String(excerpt).to_string())
}

fn json_col(row: &Row, index: usize) -> rusqlite::Result<Option<Value>> {
    let text: Option<String> = row.get(index)?;
    Ok(text.map(|t| serde_json::from_str(&t).unwrap_or(Value::String(t))))
}

fn upsert_trace(conn: &Connection, t: &TraceUpsert, now: i64) -> Result<(), String> {
    let tags = t.tags.as_ref().map(|tags| Value::from(tags.clone()).to_string());
    conn.execute(
        "INSERT INTO traces (id, source, project, name, user_id, session_id, tags, input, output, input_preview, \
             output_preview, metadata, release, version, environment, start_us, end_us, received_us, updated_us) \
         VALUES (:id, :source, :project, COALESCE(:name, :dname), :user_id, :session_id, :tags, \
             COALESCE(:input, :dinput), COALESCE(:output, :doutput), COALESCE(:ip, :dip), COALESCE(:op, :dop), \
             :metadata, :release, :version, :environment, COALESCE(:start, :end, :now), :end, :now, :now) \
         ON CONFLICT(id) DO UPDATE SET \
             project = COALESCE(traces.project, :project), \
             name = COALESCE(:name, traces.name, :dname), \
             input = COALESCE(:input, traces.input, :dinput), \
             output = COALESCE(:output, traces.output, :doutput), \
             input_preview = CASE WHEN :input IS NOT NULL THEN :ip \
                                  WHEN traces.input IS NULL AND :dinput IS NOT NULL THEN :dip \
                                  ELSE traces.input_preview END, \
             output_preview = CASE WHEN :output IS NOT NULL THEN :op \
                                   WHEN traces.output IS NULL AND :doutput IS NOT NULL THEN :dop \
                                   ELSE traces.output_preview END, \
             user_id = COALESCE(:user_id, traces.user_id), \
             session_id = COALESCE(:session_id, traces.session_id), \
             tags = COALESCE(:tags, traces.tags), \
             metadata = CASE WHEN :metadata IS NULL THEN traces.metadata \
                             WHEN traces.metadata IS NULL OR json_type(traces.metadata) != 'object' \
                               OR json_type(:metadata) != 'object' THEN :metadata \
                             ELSE json_patch(traces.metadata, :metadata) END, \
             release = COALESCE(:release, traces.release), \
             version = COALESCE(:version, traces.version), \
             environment = COALESCE(:environment, traces.environment), \
             start_us = CASE WHEN :start IS NULL THEN traces.start_us ELSE MIN(traces.start_us, :start) END, \
             end_us = CASE WHEN :end IS NULL THEN traces.end_us \
                           WHEN traces.end_us IS NULL THEN :end ELSE MAX(traces.end_us, :end) END, \
             updated_us = :now",
        named_params! {
            ":id": t.id,
            ":source": t.source,
            ":project": t.project,
            ":name": t.name,
            ":dname": t.default_name,
            ":user_id": t.user_id,
            ":session_id": t.session_id,
            ":tags": tags,
            ":input": to_json_text(&t.input),
            ":dinput": to_json_text(&t.default_input),
            ":output": to_json_text(&t.output),
            ":doutput": to_json_text(&t.default_output),
            ":ip": t.input.as_ref().and_then(preview),
            ":dip": t.default_input.as_ref().and_then(preview),
            ":op": t.output.as_ref().and_then(preview),
            ":dop": t.default_output.as_ref().and_then(preview),
            ":metadata": to_json_text(&t.metadata),
            ":release": t.release,
            ":version": t.version,
            ":environment": t.environment,
            ":start": t.start_us,
            ":end": t.end_us,
            ":now": now,
        },
    )
    .map_err(err)?;
    Ok(())
}

/// The trace an observation belongs to when its message didn't say: the one
/// its stored row already has, else its parent's, else a trace of its own.
fn resolve_trace_id(conn: &Connection, obs: &ObservationUpsert) -> Result<String, String> {
    if let Some(id) = obs.trace_id.as_deref().filter(|s| !s.is_empty()) {
        return Ok(id.to_string());
    }
    let lookup = |id: &str| -> Result<Option<String>, String> {
        conn.query_row("SELECT trace_id FROM observations WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(err)
    };
    if let Some(existing) = lookup(&obs.id)? {
        return Ok(existing);
    }
    if let Some(parent) = obs.parent_id.as_deref() {
        if let Some(found) = lookup(parent)? {
            return Ok(found);
        }
    }
    Ok(obs.id.clone())
}

fn observation_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.query_row("SELECT 1 FROM observations WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map(|found| found.is_some())
        .map_err(err)
}

/// `trace_id` is `resolve_trace_id`'s answer for `o`.
fn upsert_observation(
    conn: &Connection,
    o: &ObservationUpsert,
    trace_id: &str,
    implicit_source: &'static str,
    now: i64,
) -> Result<(), String> {
    // Every observation's trace exists, even when the SDK never sent one: the
    // stub takes the observation's times and fills in as more arrives.
    let end = o.end_us.or(o.start_us);
    upsert_trace(
        conn,
        &TraceUpsert {
            id: trace_id.to_string(),
            source: implicit_source,
            start_us: o.start_us,
            end_us: end,
            ..Default::default()
        },
        now,
    )?;
    conn.execute(
        "INSERT INTO observations (id, trace_id, parent_id, kind, name, start_us, end_us, completion_start_us, model, \
             model_parameters, input, output, metadata, input_tokens, output_tokens, total_tokens, cost, level, \
             status_message, attributes, received_us) \
         VALUES (:id, :trace_id, :parent_id, :kind, :name, :start, :end, :cstart, :model, :params, :input, :output, \
             :metadata, :in_tok, :out_tok, :total_tok, :cost, :level, :status, :attributes, :now) \
         ON CONFLICT(id) DO UPDATE SET \
             trace_id = :trace_id, \
             parent_id = COALESCE(:parent_id, observations.parent_id), \
             kind = COALESCE(:kind, observations.kind), \
             name = COALESCE(:name, observations.name), \
             start_us = COALESCE(:start, observations.start_us), \
             end_us = COALESCE(:end, observations.end_us), \
             completion_start_us = COALESCE(:cstart, observations.completion_start_us), \
             model = COALESCE(:model, observations.model), \
             model_parameters = COALESCE(:params, observations.model_parameters), \
             input = COALESCE(:input, observations.input), \
             output = COALESCE(:output, observations.output), \
             metadata = CASE WHEN :metadata IS NULL THEN observations.metadata \
                             WHEN observations.metadata IS NULL OR json_type(observations.metadata) != 'object' \
                               OR json_type(:metadata) != 'object' THEN :metadata \
                             ELSE json_patch(observations.metadata, :metadata) END, \
             input_tokens = COALESCE(:in_tok, observations.input_tokens), \
             output_tokens = COALESCE(:out_tok, observations.output_tokens), \
             total_tokens = COALESCE(:total_tok, observations.total_tokens), \
             cost = COALESCE(:cost, observations.cost), \
             level = COALESCE(:level, observations.level), \
             status_message = COALESCE(:status, observations.status_message), \
             attributes = COALESCE(:attributes, observations.attributes)",
        named_params! {
            ":id": o.id,
            ":trace_id": trace_id,
            ":parent_id": o.parent_id,
            ":kind": o.kind,
            ":name": o.name,
            ":start": o.start_us,
            ":end": o.end_us,
            ":cstart": o.completion_start_us,
            ":model": o.model,
            ":params": to_json_text(&o.model_parameters),
            ":input": to_json_text(&o.input),
            ":output": to_json_text(&o.output),
            ":metadata": to_json_text(&o.metadata),
            ":in_tok": o.input_tokens,
            ":out_tok": o.output_tokens,
            ":total_tok": o.total_tokens,
            ":cost": o.cost,
            ":level": o.level,
            ":status": o.status_message,
            ":attributes": to_json_text(&o.attributes),
            ":now": now,
        },
    )
    .map_err(err)?;
    conn.execute(
        "UPDATE scores SET trace_id = ?2 WHERE trace_id IS NULL AND observation_id = ?1",
        params![o.id, trace_id],
    )
    .map_err(err)?;
    // A root observation stands in for the trace where the SDK named nothing
    // at trace level. This also catches a root's outputs arriving later, in an
    // update that doesn't say it is the root (a LangSmith PATCH).
    conn.execute(
        "UPDATE traces SET \
             name = COALESCE(traces.name, o.name), \
             input = COALESCE(traces.input, o.input), \
             output = COALESCE(traces.output, o.output), \
             input_preview = CASE WHEN traces.input IS NULL THEN COALESCE(?3, traces.input_preview) \
                                  ELSE traces.input_preview END, \
             output_preview = CASE WHEN traces.output IS NULL THEN COALESCE(?4, traces.output_preview) \
                                   ELSE traces.output_preview END \
         FROM (SELECT name, input, output FROM observations WHERE id = ?1 AND parent_id IS NULL) AS o \
         WHERE traces.id = ?2",
        params![
            o.id,
            trace_id,
            o.input.as_ref().and_then(preview),
            o.output.as_ref().and_then(preview)
        ],
    )
    .map_err(err)?;
    Ok(())
}

/// A score names its trace, or an observation whose trace is looked up. One
/// whose observation hasn't arrived yet is kept with no trace and attached
/// when it does: LangSmith's SDK posts feedback at once but batches the runs
/// it scores, so the feedback usually comes first. One naming neither is
/// dropped, as the page has nowhere to show it.
fn upsert_score(conn: &Connection, s: &ScoreUpsert, implicit_source: &'static str, now: i64) -> Result<bool, String> {
    let trace_id = match (&s.trace_id, &s.observation_id) {
        (Some(t), _) if !t.is_empty() => Some(t.clone()),
        (_, Some(o)) => conn
            .query_row("SELECT trace_id FROM observations WHERE id = ?1", [o], |r| r.get::<_, String>(0))
            .optional()
            .map_err(err)?,
        _ => None,
    };
    if trace_id.is_none() && s.observation_id.is_none() {
        return Ok(false);
    }
    if let Some(trace_id) = &trace_id {
        upsert_trace(conn, &TraceUpsert { id: trace_id.clone(), source: implicit_source, ..Default::default() }, now)?;
    }
    conn.execute(
        "INSERT OR REPLACE INTO scores (id, trace_id, observation_id, name, value, string_value, comment, time_us) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![s.id, trace_id, s.observation_id, s.name, s.value, s.string_value, s.comment, s.time_us.unwrap_or(now)],
    )
    .map_err(err)?;
    Ok(true)
}

fn prune(conn: &Connection, limits: Limits) -> Result<(), String> {
    // Scores still waiting for an observation after a day never will get one,
    // and only so many are kept waiting at all.
    conn.execute(
        "DELETE FROM scores WHERE trace_id IS NULL AND time_us < ?1",
        [now_us() - 86_400_000_000],
    )
    .map_err(err)?;
    conn.execute(
        "DELETE FROM scores WHERE trace_id IS NULL AND id NOT IN \
         (SELECT id FROM scores WHERE trace_id IS NULL ORDER BY time_us DESC LIMIT ?1)",
        [MAX_WAITING_SCORES],
    )
    .map_err(err)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM traces", [], |r| r.get(0)).map_err(err)?;
    if count > i64::from(limits.traces) {
        conn.execute(
            "DELETE FROM traces WHERE id NOT IN (SELECT id FROM traces ORDER BY start_us DESC LIMIT ?1)",
            [limits.traces],
        )
        .map_err(err)?;
        conn.execute_batch(
            "DELETE FROM observations WHERE trace_id NOT IN (SELECT id FROM traces);
             DELETE FROM scores WHERE trace_id NOT IN (SELECT id FROM traces);",
        )
        .map_err(err)?;
    }
    // Over the size budget, the oldest tenth goes until it fits. Deleted
    // pages are free for reuse at once, so the file stops growing here,
    // though it doesn't shrink until "Clear all" vacuums it.
    while used_bytes(conn)? > limits.bytes {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM traces", [], |r| r.get(0)).map_err(err)?;
        let oldest: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM traces ORDER BY start_us ASC LIMIT ?1").map_err(err)?;
            let ids = stmt.query_map([(count / 10).max(1)], |r| r.get(0)).map_err(err)?;
            ids.collect::<Result<_, _>>().map_err(err)?
        };
        if oldest.is_empty() {
            break;
        }
        for id in &oldest {
            conn.execute("DELETE FROM observations WHERE trace_id = ?1", [id]).map_err(err)?;
            conn.execute("DELETE FROM scores WHERE trace_id = ?1", [id]).map_err(err)?;
            conn.execute("DELETE FROM traces WHERE id = ?1", [id]).map_err(err)?;
        }
    }
    Ok(())
}

/// The database's size less its free pages, as of this transaction.
fn used_bytes(conn: &Connection) -> Result<u64, String> {
    let pragma = |name: &str| conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0)).map_err(err);
    let pages = pragma("page_count")? - pragma("freelist_count")?;
    Ok(pages.max(0) as u64 * pragma("page_size")?.max(0) as u64)
}

/// Columns 0–11 of the list query.
fn summary_from_row(r: &Row) -> rusqlite::Result<TraceSummary> {
    let tags: Option<String> = r.get(6)?;

    Ok(TraceSummary {
        id: r.get(0)?,
        source: r.get(1)?,
        project: r.get(2)?,
        name: r.get(3)?,
        user_id: r.get(4)?,
        session_id: r.get(5)?,
        tags: tags.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        start_us: r.get(7)?,
        end_us: r.get(8)?,
        updated_us: r.get(9)?,
        observation_count: 0,
        generation_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost: None,
        error_count: 0,
        models: Vec::new(),
        input_preview: r.get(10)?,
        output_preview: r.get(11)?,
    })
}

/// Counts, token totals, cost and models per trace, for one page of traces.
/// Tokens are summed over LLM calls (`generation`, `embedding`) only: the
/// wrappers around them — an agent span, a chain — often repeat their
/// children's usage, and counting both would double it.
fn fill_aggregates(conn: &Connection, traces: &mut [TraceSummary]) -> Result<(), String> {
    if traces.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; traces.len()].join(",");
    let sql = format!(
        "SELECT trace_id, COUNT(*), \
             SUM(kind IN ('generation','embedding')), \
             SUM(CASE WHEN kind IN ('generation','embedding') THEN COALESCE(input_tokens, 0) END), \
             SUM(CASE WHEN kind IN ('generation','embedding') THEN COALESCE(output_tokens, 0) END), \
             SUM(CASE WHEN kind IN ('generation','embedding') \
                 THEN COALESCE(total_tokens, COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) END), \
             SUM(cost), SUM(level = 'ERROR'), GROUP_CONCAT(DISTINCT model) \
         FROM observations WHERE trace_id IN ({placeholders}) GROUP BY trace_id"
    );
    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let ids: Vec<&str> = traces.iter().map(|t| t.id.as_str()).collect();
    let rows = stmt
        .query_map(params_from_iter(ids.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                (
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    r.get::<_, Option<f64>>(6)?,
                    r.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    r.get::<_, Option<String>>(8)?,
                ),
            ))
        })
        .map_err(err)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(err)?;
    for trace in traces.iter_mut() {
        if let Some((count, generations, input, output, total, cost, errors, models)) = rows.get(&trace.id) {
            trace.observation_count = *count;
            trace.generation_count = *generations;
            trace.input_tokens = *input;
            trace.output_tokens = *output;
            trace.total_tokens = *total;
            trace.cost = *cost;
            trace.error_count = *errors;
            trace.models = models
                .as_deref()
                .map(|m| m.split(',').map(str::to_string).collect())
                .unwrap_or_default();
        }
    }
    Ok(())
}

/// A one-line taste of a trace's input or output for the list: the last user
/// message of a chat, the question of a `{question: …}` object, or the text
/// itself; compact JSON when none of those fit.
fn preview(value: &Value) -> Option<String> {
    let text = text_of(value, 0).unwrap_or_else(|| value.to_string());
    let squashed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.is_empty() || squashed == "null" {
        return None;
    }
    Some(if squashed.chars().count() > PREVIEW_CHARS {
        let cut: String = squashed.chars().take(PREVIEW_CHARS).collect();
        format!("{cut}…")
    } else {
        squashed
    })
}

fn text_of(value: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            // A chat: prefer the last user turn, else the last message.
            let user = items.iter().rev().find(|m| {
                m.get("role").and_then(Value::as_str) == Some("user")
                    || m.get("type").and_then(Value::as_str) == Some("human")
            });
            user.or_else(|| items.last()).and_then(|m| text_of(m, depth + 1))
        }
        Value::Object(map) => {
            for key in ["content", "text", "input", "query", "question", "prompt", "message", "messages", "output", "kwargs"] {
                if let Some(found) = map.get(key).and_then(|v| text_of(v, depth + 1)) {
                    return Some(found);
                }
            }
            if map.len() == 1 {
                return map.values().next().and_then(|v| text_of(v, depth + 1));
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obs(id: &str, trace: Option<&str>, parent: Option<&str>) -> ObservationUpsert {
        ObservationUpsert {
            id: id.into(),
            trace_id: trace.map(Into::into),
            parent_id: parent.map(Into::into),
            ..Default::default()
        }
    }

    fn ingest(store: &Store, batch: Batch) {
        store.ingest(&batch, "otel", 1000).unwrap();
    }

    #[test]
    fn create_then_update_merges_into_one_row() {
        let store = Store::open_in_memory();
        let mut create = obs("o1", Some("t1"), None);
        create.kind = Some("generation".into());
        create.name = Some("chat".into());
        create.start_us = Some(1_000);
        create.input = Some(json!([{"role": "user", "content": "hi"}]));
        create.metadata = Some(json!({"a": 1}));
        ingest(&store, Batch { observations: vec![create], ..Default::default() });

        let mut update = obs("o1", Some("t1"), None);
        update.end_us = Some(5_000);
        update.output = Some(json!("hello"));
        update.input_tokens = Some(3);
        update.output_tokens = Some(2);
        update.metadata = Some(json!({"b": 2}));
        ingest(&store, Batch { observations: vec![update], ..Default::default() });

        let detail = store.get("t1").unwrap().unwrap();
        assert_eq!(detail.observations.len(), 1);
        let o = &detail.observations[0];
        assert_eq!(o.name.as_deref(), Some("chat"));
        assert_eq!(o.kind, "generation");
        assert_eq!((o.start_us, o.end_us), (Some(1_000), Some(5_000)));
        assert_eq!(o.input, Some(json!([{"role": "user", "content": "hi"}])));
        assert_eq!(o.output, Some(json!("hello")));
        assert_eq!(o.metadata, Some(json!({"a": 1, "b": 2})));
        assert_eq!(detail.summary.total_tokens, 5);
        assert_eq!((detail.summary.start_us, detail.summary.end_us), (1_000, Some(5_000)));
    }

    #[test]
    fn an_update_without_a_trace_id_finds_its_row_and_a_child_finds_its_parent() {
        let store = Store::open_in_memory();
        ingest(&store, Batch { observations: vec![obs("root", Some("t9"), None)], ..Default::default() });
        let mut update = obs("root", None, None);
        update.name = Some("renamed".into());
        ingest(&store, Batch { observations: vec![update, obs("child", None, Some("root"))], ..Default::default() });

        let detail = store.get("t9").unwrap().unwrap();
        let ids: Vec<&str> = detail.observations.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"child"));
        assert_eq!(detail.observations.iter().find(|o| o.id == "root").unwrap().name.as_deref(), Some("renamed"));
        // An orphan with nothing to go on becomes a trace of its own.
        ingest(&store, Batch { observations: vec![obs("lonely", None, None)], ..Default::default() });
        assert!(store.get("lonely").unwrap().is_some());
    }

    #[test]
    fn explicit_trace_fields_beat_root_span_defaults_in_either_order() {
        let store = Store::open_in_memory();
        let default = TraceUpsert { id: "t".into(), source: "otel", default_name: Some("root span".into()), ..Default::default() };
        let explicit = TraceUpsert { id: "t".into(), source: "otel", name: Some("my trace".into()), ..Default::default() };
        ingest(&store, Batch { traces: vec![default.clone(), explicit.clone()], ..Default::default() });
        assert_eq!(store.get("t").unwrap().unwrap().summary.name.as_deref(), Some("my trace"));

        let store = Store::open_in_memory();
        ingest(&store, Batch { traces: vec![explicit, default], ..Default::default() });
        assert_eq!(store.get("t").unwrap().unwrap().summary.name.as_deref(), Some("my trace"));
    }

    #[test]
    fn tokens_count_llm_calls_only_so_wrappers_do_not_double_them() {
        let store = Store::open_in_memory();
        let mut agent = obs("agent", Some("t"), None);
        agent.kind = Some("agent".into());
        agent.total_tokens = Some(100);
        let mut llm = obs("llm", Some("t"), Some("agent"));
        llm.kind = Some("generation".into());
        llm.model = Some("gpt-x".into());
        llm.total_tokens = Some(100);
        llm.level = Some("ERROR".into());
        ingest(&store, Batch { observations: vec![agent, llm], ..Default::default() });
        let list = store.list(&ListQuery::default()).unwrap();
        assert_eq!(list.total, 1);
        let t = &list.traces[0];
        assert_eq!((t.total_tokens, t.generation_count, t.observation_count, t.error_count), (100, 1, 2, 1));
        assert_eq!(t.models, vec!["gpt-x".to_string()]);
    }

    #[test]
    fn list_filters_by_search_source_project_and_errors() {
        let store = Store::open_in_memory();
        for (id, source, project, name) in [("a", "langfuse", "p1", "checkout"), ("b", "langsmith", "p2", "search docs")] {
            ingest(
                &store,
                Batch {
                    traces: vec![TraceUpsert {
                        id: id.into(),
                        source,
                        project: Some(project.into()),
                        name: Some(name.into()),
                        input: Some(json!({"question": format!("about {name}")})),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
        }
        let q = |search: Option<&str>, source: Option<&str>, project: Option<&str>| ListQuery {
            search: search.map(Into::into),
            source: source.map(Into::into),
            project: project.map(Into::into),
            ..Default::default()
        };
        let ids = |q: ListQuery| store.list(&q).unwrap().traces.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(q(Some("docs"), None, None)), vec!["b"]);
        assert_eq!(ids(q(None, Some("langfuse"), None)), vec!["a"]);
        assert_eq!(ids(q(None, None, Some("p2"))), vec!["b"]);
        assert_eq!(ids(q(Some("100%"), None, None)), Vec::<String>::new());
        assert_eq!(store.list(&ListQuery { errors_only: true, ..Default::default() }).unwrap().total, 0);
        let list = store.list(&ListQuery::default()).unwrap();
        assert_eq!(list.projects, vec!["p1", "p2"]);
        assert_eq!(list.traces.iter().find(|t| t.id == "a").unwrap().input_preview.as_deref(), Some("about checkout"));
    }

    #[test]
    fn a_roots_late_output_reaches_the_trace() {
        let store = Store::open_in_memory();
        let mut root = obs("r", Some("r"), None);
        root.name = Some("pipeline".into());
        root.input = Some(json!({"q": 1}));
        ingest(&store, Batch { observations: vec![root], ..Default::default() });
        let mut patch = obs("r", None, None);
        patch.output = Some(json!({"a": 2}));
        ingest(&store, Batch { observations: vec![patch], ..Default::default() });
        let summary = store.get("r").unwrap().unwrap();
        assert_eq!(summary.summary.name.as_deref(), Some("pipeline"));
        assert_eq!((summary.input, summary.output), (Some(json!({"q": 1})), Some(json!({"a": 2}))));
    }

    #[test]
    fn scores_attach_through_their_observation() {
        let store = Store::open_in_memory();
        ingest(&store, Batch { observations: vec![obs("o", Some("t"), None)], ..Default::default() });
        let score = ScoreUpsert {
            id: "s".into(),
            observation_id: Some("o".into()),
            name: "correct".into(),
            value: Some(1.0),
            ..Default::default()
        };
        let lost = ScoreUpsert { id: "x".into(), name: "orphan".into(), ..Default::default() };
        let counts = store.ingest(&Batch { scores: vec![score, lost], ..Default::default() }, "otel", 10).unwrap();
        assert_eq!(counts.scores, 1);
        assert_eq!(store.get("t").unwrap().unwrap().scores[0].name, "correct");
    }

    #[test]
    fn a_score_that_arrives_before_its_run_is_attached_when_the_run_does() {
        let store = Store::open_in_memory();
        let early = ScoreUpsert {
            id: "fb".into(),
            observation_id: Some("run".into()),
            name: "correctness".into(),
            value: Some(1.0),
            ..Default::default()
        };
        store.ingest(&Batch { scores: vec![early], ..Default::default() }, "langsmith", 10).unwrap();
        ingest(&store, Batch { observations: vec![obs("run", Some("run"), None)], ..Default::default() });
        let detail = store.get("run").unwrap().unwrap();
        assert_eq!(detail.scores.len(), 1);
        assert_eq!(detail.scores[0].name, "correctness");
    }

    #[test]
    fn the_store_keeps_only_the_newest_traces() {
        let store = Store::open_in_memory();
        for i in 0..5 {
            let mut o = obs(&format!("o{i}"), Some(&format!("t{i}")), None);
            o.start_us = Some(i * 1000);
            store.ingest(&Batch { observations: vec![o], ..Default::default() }, "otel", 3).unwrap();
        }
        let list = store.list(&ListQuery::default()).unwrap();
        assert_eq!(list.traces.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t4", "t3", "t2"]);
        assert_eq!(store.stats().unwrap().observations, 3);
    }

    #[test]
    fn a_full_trace_takes_updates_but_no_new_observations() {
        let store = Store::open_in_memory();
        let limits = Limits { traces: 1000, observations_per_trace: 3, bytes: u64::MAX };
        let batch = |ids: &[&str]| Batch { observations: ids.iter().map(|id| obs(id, Some("t"), None)).collect(), ..Default::default() };
        let counts = store.ingest_within(&batch(&["a", "b", "c", "d", "e"]), "otel", limits).unwrap();
        assert_eq!((counts.observations, counts.dropped), (3, 2));

        let mut update = obs("a", Some("t"), None);
        update.output = Some(json!("done"));
        let counts = store
            .ingest_within(&Batch { observations: vec![update, obs("f", Some("t"), None)], ..Default::default() }, "otel", limits)
            .unwrap();
        assert_eq!((counts.observations, counts.dropped), (1, 1));

        let detail = store.get("t").unwrap().unwrap();
        assert_eq!(detail.observations.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
        assert_eq!(detail.observations[0].output, Some(json!("done")));
        // Another trace has room of its own.
        let counts = store.ingest_within(&Batch { observations: vec![obs("g", Some("u"), None)], ..Default::default() }, "otel", limits).unwrap();
        assert_eq!(counts.observations, 1);
    }

    #[test]
    fn past_the_size_budget_the_oldest_traces_go() {
        let store = Store::open_in_memory();
        let budget = 2 << 20;
        let limits = Limits { traces: 1000, observations_per_trace: 1000, bytes: budget };
        // 40 traces of about 200 kB each (a root's input is kept on the trace
        // too): four times the budget.
        for i in 0..40 {
            let mut o = obs(&format!("o{i}"), Some(&format!("t{i:02}")), None);
            o.start_us = Some(i * 1000);
            o.input = Some(Value::String(format!("{i:02}").repeat(50_000)));
            store.ingest_within(&Batch { observations: vec![o], ..Default::default() }, "otel", limits).unwrap();
        }
        assert!(used_bytes(&store.lock()).unwrap() <= budget);
        let list = store.list(&ListQuery { limit: Some(100), ..Default::default() }).unwrap();
        let ids: Vec<_> = list.traces.iter().map(|t| t.id.clone()).collect();
        assert!((5..40).contains(&ids.len()), "{} traces kept", ids.len());
        // The newest ones, with no gaps: the oldest went first.
        let newest: Vec<_> = (40 - ids.len()..40).rev().map(|i| format!("t{i:02}")).collect();
        assert_eq!(ids, newest);
        assert_eq!(store.stats().unwrap().observations, ids.len() as i64);
    }

    #[test]
    fn only_so_many_scores_wait_for_their_run() {
        let store = Store::open_in_memory();
        let waiting = |i: u32| ScoreUpsert {
            id: format!("s{i}"),
            observation_id: Some(format!("run{i}")),
            name: "n".into(),
            time_us: Some(now_us() + i64::from(i)),
            ..Default::default()
        };
        let scores = (0..MAX_WAITING_SCORES + 5).map(waiting).collect();
        store.ingest(&Batch { scores, ..Default::default() }, "langsmith", 10).unwrap();
        let conn = store.lock();
        let kept: i64 = conn.query_row("SELECT COUNT(*) FROM scores", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, i64::from(MAX_WAITING_SCORES));
        let oldest_gone: bool =
            conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM scores WHERE id = 's0')", [], |r| r.get(0)).unwrap();
        assert!(oldest_gone);
    }

    #[test]
    fn oversized_fields_are_cut_to_a_marked_excerpt() {
        let big = Some(Value::String("x".repeat(MAX_FIELD_BYTES + 10)));
        let text = to_json_text(&big).unwrap();
        assert!(text.len() < EXCERPT_BYTES + 200);
        assert!(text.contains("DevsPro kept the first"));
        let small = Some(json!({"a": 1}));
        assert_eq!(to_json_text(&small).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn previews_pick_the_human_part() {
        let p = |v: Value| preview(&v);
        assert_eq!(
            p(json!([{"role": "system", "content": "be nice"}, {"role": "user", "content": "what is 2+2?"}])).as_deref(),
            Some("what is 2+2?")
        );
        assert_eq!(p(json!({"messages": [{"role": "user", "content": "hey"}]})).as_deref(), Some("hey"));
        assert_eq!(p(json!("plain")).as_deref(), Some("plain"));
        assert_eq!(p(json!({"a": 1, "b": 2})).as_deref(), Some(r#"{"a":1,"b":2}"#));
        assert_eq!(p(Value::Null), None);
        assert!(p(json!("y".repeat(500))).unwrap().ends_with('…'));
    }
}
