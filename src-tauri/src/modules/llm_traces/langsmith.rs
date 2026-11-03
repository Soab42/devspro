//! LangSmith's run API, as its SDKs use it for tracing:
//!
//! - `POST /runs` and `PATCH /runs/{id}`: one run at a time (older SDKs);
//! - `POST /runs/batch`: `{"post": [run…], "patch": [run…]}`;
//! - `POST /runs/multipart`: the same, as multipart/form-data with one part per
//!   run and one per large field (`post.<id>`, `post.<id>.inputs`,
//!   `patch.<id>.outputs`, `attachment.<id>.<name>`…). Current SDKs use it
//!   when `/info` says the server supports it, which DevsPro's does;
//! - `POST /feedback`: a score on a run.
//!
//! A run is LangSmith's word for an observation, and the root run of a tree
//! is the trace itself: its name, inputs and outputs are the trace's. Runs
//! name their project in `session_name` (`LANGSMITH_PROJECT`).

use super::model::{
    as_string, parse_time_us, present, usage_from, Batch, ObservationUpsert, ScoreUpsert, TraceUpsert,
    SOURCE_LANGSMITH,
};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A run being created (`post`) or finished (`patch`). Only a create without
/// a parent may be taken for a root: a patch leaves `parent_run_id` out
/// whether or not the run has one.
pub fn run_to_batch(run: &Value, is_create: bool, batch: &mut Batch) -> Result<(), String> {
    let id = run.get("id").and_then(as_string).ok_or("run has no id")?;
    let parent_id = run.get("parent_run_id").and_then(as_string);
    let dotted = run.get("dotted_order").and_then(Value::as_str);
    let trace_id = run
        .get("trace_id")
        .and_then(as_string)
        .or_else(|| dotted.and_then(trace_id_from_dotted_order))
        // A run with no parent is its own trace's root.
        .or_else(|| (is_create && parent_id.is_none()).then(|| id.clone()));
    let extra = run.get("extra");
    let metadata = extra.and_then(|e| e.get("metadata")).filter(|m| m.is_object());
    let invocation = extra.and_then(|e| e.get("invocation_params")).filter(|m| m.is_object());
    let inputs = present(run.get("inputs")).filter(|v| !is_empty_object(v));
    let outputs = present(run.get("outputs")).filter(|v| !is_empty_object(v));
    let error = run.get("error").and_then(as_string);
    let start_us = run.get("start_time").and_then(parse_time_us);
    let end_us = run.get("end_time").and_then(parse_time_us);
    let kind = run.get("run_type").and_then(Value::as_str).map(|t| match t {
        "llm" => "generation".to_string(),
        other => other.to_lowercase(),
    });

    let usage = find_usage(outputs.as_ref(), 0).or_else(|| metadata.and_then(|m| m.get("usage_metadata")).cloned());
    let (input_tokens, output_tokens, total_tokens) = usage.as_ref().map(usage_from).unwrap_or_default();
    let model = metadata
        .and_then(|m| m.get("ls_model_name"))
        .or_else(|| invocation.and_then(|i| i.get("model").or_else(|| i.get("model_name"))))
        .or_else(|| inputs.as_ref().and_then(|i| i.get("model")))
        .or_else(|| outputs.as_ref().and_then(|o| o.get("model")))
        .or_else(|| outputs.as_ref().and_then(|o| o.get("llm_output")).and_then(|l| l.get("model_name")))
        .and_then(as_string)
        // Only an LLM run calls a model; a chain that passes `model` along doesn't.
        .filter(|_| matches!(kind.as_deref(), Some("generation") | Some("embedding") | None));
    let first_token = run
        .get("events")
        .and_then(Value::as_array)
        .and_then(|events| events.iter().find(|e| e.get("name").and_then(Value::as_str) == Some("new_token")))
        .and_then(|e| e.get("time"))
        .and_then(parse_time_us);

    let mut obs_metadata = metadata.and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(tags) = run.get("tags").filter(|t| t.as_array().is_some_and(|a| !a.is_empty())) {
        obs_metadata.insert("tags".into(), tags.clone());
    }

    let project = run.get("session_name").and_then(as_string);
    let is_root = parent_id.is_none() && trace_id.as_deref().is_some_and(|t| t == id);
    if let Some(trace_id) = trace_id.clone() {
        let mut trace = TraceUpsert {
            id: trace_id,
            source: SOURCE_LANGSMITH,
            project,
            start_us,
            end_us,
            ..Default::default()
        };
        if is_root {
            trace.name = run.get("name").and_then(as_string);
            trace.input = inputs.clone();
            trace.output = outputs.clone();
            trace.tags = run.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(as_string).collect());
            trace.metadata = metadata.cloned();
            let meta = |keys: &[&str]| keys.iter().find_map(|k| metadata.and_then(|m| m.get(*k)).and_then(as_string));
            trace.user_id = meta(&["user_id", "userId"]);
            trace.session_id = meta(&["session_id", "thread_id", "conversation_id"]);
            trace.environment = meta(&["env", "environment", "LANGCHAIN_ENV"]);
            trace.release = meta(&["revision_id"]);
        }
        batch.traces.push(trace);
    }

    batch.observations.push(ObservationUpsert {
        id,
        trace_id,
        parent_id,
        kind,
        name: run.get("name").and_then(as_string),
        start_us,
        end_us,
        completion_start_us: first_token,
        model,
        model_parameters: invocation.cloned(),
        input: inputs,
        output: outputs,
        metadata: (!obs_metadata.is_empty()).then_some(Value::Object(obs_metadata)),
        input_tokens,
        output_tokens,
        total_tokens,
        cost: usage.as_ref().and_then(|u| u.get("total_cost")).and_then(Value::as_f64),
        level: error.as_ref().map(|_| "ERROR".to_string()),
        status_message: error,
        attributes: None,
    });
    Ok(())
}

fn is_empty_object(v: &Value) -> bool {
    v.as_object().is_some_and(Map::is_empty)
}

/// `dotted_order` is `<time>Z<trace id>.<time>Z<child id>…`; the first
/// segment's id is the trace's.
fn trace_id_from_dotted_order(dotted: &str) -> Option<String> {
    let first = dotted.split('.').next()?;
    let (_, id) = first.split_once('Z')?;
    (!id.is_empty()).then(|| id.to_string())
}

/// Token usage wherever this generation of LangChain or the SDK wrappers left
/// it: `usage_metadata` on the outputs or on the message inside them,
/// `llm_output.token_usage`, or OpenAI's `usage`. A shallow search, not a
/// fixed path, because each integration nests it differently.
fn find_usage(value: Option<&Value>, depth: usize) -> Option<Value> {
    let value = value?;
    if depth > 6 {
        return None;
    }
    match value {
        Value::Object(map) => {
            for key in ["usage_metadata", "token_usage", "usage"] {
                if let Some(found) = map.get(key).filter(|u| usage_from(u) != (None, None, None)) {
                    return Some(found.clone());
                }
            }
            map.values().find_map(|v| find_usage(Some(v), depth + 1))
        }
        Value::Array(items) => items.iter().take(4).find_map(|v| find_usage(Some(v), depth + 1)),
        _ => None,
    }
}

/// `POST /runs/batch` and the parsed parts of `POST /runs/multipart`. Creates
/// go first, so a patch in the same request finds its run.
pub fn batch_to_batch(posts: &[Value], patches: &[Value]) -> Result<Batch, String> {
    let mut batch = Batch::default();
    for run in posts {
        run_to_batch(run, true, &mut batch)?;
    }
    for run in patches {
        run_to_batch(run, false, &mut batch)?;
    }
    Ok(batch)
}

/// `PATCH /runs/{id}`: the id is in the path, and often not in the body.
pub fn patch_to_batch(id: &str, body: &Value) -> Result<Batch, String> {
    let mut run = body.as_object().cloned().unwrap_or_default();
    run.insert("id".into(), Value::String(id.to_string()));
    let mut batch = Batch::default();
    run_to_batch(&Value::Object(run), false, &mut batch)?;
    Ok(batch)
}

pub fn feedback_to_score(body: &Value) -> Result<ScoreUpsert, String> {
    let name = body.get("key").and_then(as_string).ok_or("feedback has no key")?;
    let score = body.get("score");
    let value = body.get("value");
    Ok(ScoreUpsert {
        id: body.get("id").and_then(as_string).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        trace_id: body.get("trace_id").and_then(as_string),
        observation_id: body.get("run_id").and_then(as_string),
        name,
        value: score
            .and_then(Value::as_f64)
            .or_else(|| score.and_then(Value::as_bool).map(|b| if b { 1.0 } else { 0.0 }))
            .or_else(|| value.and_then(Value::as_f64)),
        string_value: value.and_then(|v| v.as_str().map(str::to_string)),
        comment: body.get("comment").and_then(as_string),
        time_us: body.get("created_at").and_then(parse_time_us),
    })
}

// ---------------------------------------------------------------------------
// multipart/form-data

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

pub fn boundary(content_type: &str) -> Option<String> {
    content_type.split(';').map(str::trim).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        key.trim().eq_ignore_ascii_case("boundary").then(|| value.trim().trim_matches('"').to_string())
    })
}

/// The whole body is in memory already, so this is a scan for the boundary
/// rather than a streaming parser.
pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<Part>, String> {
    let delimiter = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    let mut pos = find(body, &delimiter, 0).ok_or("multipart body has no boundary")? + delimiter.len();
    loop {
        if body[pos..].starts_with(b"--") {
            break; // the closing delimiter
        }
        pos = skip_crlf(body, pos);
        let header_end = find(body, b"\r\n\r\n", pos).ok_or("multipart part has no header end")?;
        let headers = String::from_utf8_lossy(&body[pos..header_end]).to_string();
        let data_start = header_end + 4;
        let mut next_delimiter = b"\r\n".to_vec();
        next_delimiter.extend_from_slice(&delimiter);
        let data_end = find(body, &next_delimiter, data_start).ok_or("multipart part is not terminated")?;

        let mut name = None;
        let mut content_type = None;
        for line in headers.split("\r\n") {
            let Some((key, value)) = line.split_once(':') else { continue };
            match key.trim().to_ascii_lowercase().as_str() {
                "content-disposition" => {
                    name = value.split(';').map(str::trim).find_map(|p| {
                        let (k, v) = p.split_once('=')?;
                        (k.trim() == "name").then(|| v.trim().trim_matches('"').to_string())
                    })
                }
                "content-type" => content_type = Some(value.trim().to_string()),
                _ => {}
            }
        }
        if let Some(name) = name {
            parts.push(Part { name, content_type, data: body[data_start..data_end].to_vec() });
        }
        pos = data_end + next_delimiter.len();
        if pos >= body.len() {
            break;
        }
    }
    Ok(parts)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

fn skip_crlf(body: &[u8], pos: usize) -> usize {
    if body[pos..].starts_with(b"\r\n") {
        pos + 2
    } else {
        pos
    }
}

/// Reassembles runs from their parts: `post.<id>` is the run, and
/// `post.<id>.<field>` one of its large fields sent separately. Attachments
/// (`attachment.<id>.<name>`: images, PDFs) aren't kept; the run notes their
/// names so the page can say what was left out. Feedback parts become scores.
pub fn multipart_to_batch(parts: &[Part]) -> Result<Batch, String> {
    let mut posts = Runs::default();
    let mut patches = Runs::default();
    let mut scores = Vec::new();
    for part in parts {
        let mut segments = part.name.splitn(3, '.');
        let (Some(verb), Some(id)) = (segments.next(), segments.next()) else { continue };
        let field = segments.next();
        if verb == "attachment" {
            let run = posts.entry(id);
            let names = run.entry("attachments_omitted").or_insert_with(|| Value::Array(Vec::new()));
            if let (Value::Array(list), Some(field)) = (names, field) {
                list.push(Value::String(field.to_string()));
            }
            continue;
        }
        let value: Value = serde_json::from_slice(&part.data)
            .map_err(|e| format!("part {} is not JSON: {e}", part.name))?;
        match verb {
            "feedback" => scores.push(feedback_to_score(&value)?),
            "post" | "patch" => {
                let runs = if verb == "post" { &mut posts } else { &mut patches };
                let run = runs.entry(id);
                match field {
                    None => {
                        if let Value::Object(fields) = value {
                            run.extend(fields);
                        }
                    }
                    Some(field) => {
                        run.insert(field.to_string(), value);
                    }
                }
            }
            _ => {}
        }
    }
    let finish = |runs: Runs| -> Vec<Value> {
        runs.order
            .into_iter()
            .map(|(id, mut run)| {
                run.entry("id").or_insert(Value::String(id));
                if let Some(omitted) = run.remove("attachments_omitted") {
                    let extra = run.entry("extra").or_insert_with(|| Value::Object(Map::new()));
                    if let Some(extra) = extra.as_object_mut() {
                        let metadata = extra.entry("metadata").or_insert_with(|| Value::Object(Map::new()));
                        if let Some(metadata) = metadata.as_object_mut() {
                            metadata.insert("attachments_not_stored".into(), omitted);
                        }
                    }
                }
                Value::Object(run)
            })
            .collect()
    };
    let mut batch = batch_to_batch(&finish(posts), &finish(patches))?;
    batch.scores.extend(scores);
    Ok(batch)
}

/// Runs being reassembled, in the order their first part arrived. Found by
/// id through `index`: a body can hold hundreds of thousands of parts, and a
/// scan of the list for each one would take minutes on them.
#[derive(Default)]
struct Runs {
    order: Vec<(String, Map<String, Value>)>,
    index: HashMap<String, usize>,
}

impl Runs {
    fn entry(&mut self, id: &str) -> &mut Map<String, Value> {
        let index = match self.index.get(id) {
            Some(&index) => index,
            None => {
                self.order.push((id.to_string(), Map::new()));
                self.index.insert(id.to_string(), self.order.len() - 1);
                self.order.len() - 1
            }
        };
        &mut self.order[index].1
    }
}

/// `GET /info`. The SDKs read `batch_ingest_config` to size their batches
/// and to decide on the multipart endpoint, and the compression flags to
/// decide whether to zstd (Python) or gzip (JS) the body. zstd is on: the
/// Python SDK warns on every run when it isn't, and `server` decodes it. JS's
/// gzip flag stays off, so its streamed bodies arrive plain. Python replaces its defaults with this object
/// wholesale rather than merging, so every field it indexes must be present.
pub fn info() -> Value {
    serde_json::json!({
        "version": "0.10.0",
        "license_expiration_time": null,
        "batch_ingest_config": {
            "use_multipart_endpoint": true,
            "scale_up_qsize_trigger": 1000,
            "scale_up_nthreads_limit": 16,
            "scale_down_nempty_trigger": 4,
            "size_limit": 100,
            "size_limit_bytes": 20_971_520
        },
        "instance_flags": {
            "zstd_compression_enabled": true,
            "gzip_body_enabled": false,
            "dataset_examples_multipart_enabled": false,
            "examples_multipart_enabled": false,
            "experimental_search_enabled": false,
            "generate_ai_query_enabled": false,
            "org_creation_disabled": true,
            "show_ttl_ui": false,
            "trace_tier_duration_days": {"longlived": 400, "shortlived": 14},
            "workspace_scope_org_invites": false
        },
        "tenant_handle": null
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TRACE: &str = "11111111-1111-1111-1111-111111111111";
    const CHILD: &str = "22222222-2222-2222-2222-222222222222";

    fn root_run() -> Value {
        json!({
            "id": TRACE, "trace_id": TRACE, "name": "rag_pipeline", "run_type": "chain",
            "dotted_order": format!("20240501T100000000000Z{TRACE}"),
            "start_time": "2024-05-01T10:00:00.000000", "session_name": "my-project",
            "inputs": {"question": "What is DevsPro?"}, "tags": ["beta"],
            "extra": {"metadata": {"user_id": "u7", "thread_id": "th1"}}
        })
    }

    fn llm_run() -> Value {
        json!({
            "id": CHILD, "parent_run_id": TRACE, "name": "ChatOpenAI", "run_type": "llm",
            "dotted_order": format!("20240501T100000000000Z{TRACE}.20240501T100000100000Z{CHILD}"),
            "start_time": 1714557600100i64, "session_name": "my-project",
            "inputs": {"messages": [{"role": "user", "content": "What is DevsPro?"}]},
            "extra": {"invocation_params": {"model": "gpt-4o-mini", "temperature": 0},
                      "metadata": {"ls_model_name": "gpt-4o-mini", "ls_provider": "openai"}}
        })
    }

    #[test]
    fn a_root_run_is_the_trace_and_a_child_llm_run_a_generation() {
        let patch = json!({
            "id": CHILD, "trace_id": TRACE, "end_time": "2024-05-01T10:00:01.000000",
            "outputs": {"generations": [[{"text": "A local dev tool", "message": {"kwargs": {
                "content": "A local dev tool",
                "usage_metadata": {"input_tokens": 11, "output_tokens": 4, "total_tokens": 15}}}}]]},
            "events": [{"name": "start", "time": "2024-05-01T10:00:00.100000"},
                       {"name": "new_token", "time": "2024-05-01T10:00:00.400000"}]
        });
        let batch = batch_to_batch(&[root_run(), llm_run()], &[patch]).unwrap();
        assert_eq!(batch.observations.len(), 3);

        let root_trace = &batch.traces[0];
        assert_eq!(root_trace.id, TRACE);
        assert_eq!(root_trace.name.as_deref(), Some("rag_pipeline"));
        assert_eq!(root_trace.project.as_deref(), Some("my-project"));
        assert_eq!(root_trace.input, Some(json!({"question": "What is DevsPro?"})));
        assert_eq!((root_trace.user_id.as_deref(), root_trace.session_id.as_deref()), (Some("u7"), Some("th1")));
        assert_eq!(root_trace.tags, Some(vec!["beta".to_string()]));

        let llm = &batch.observations[1];
        assert_eq!(llm.trace_id.as_deref(), Some(TRACE)); // from dotted_order
        assert_eq!(llm.kind.as_deref(), Some("generation"));
        assert_eq!(llm.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(llm.start_us, Some(1_714_557_600_100_000));
        // A child run names no trace-level fields.
        assert_eq!(batch.traces[1].name, None);

        let finished = &batch.observations[2];
        assert_eq!((finished.input_tokens, finished.output_tokens, finished.total_tokens), (Some(11), Some(4), Some(15)));
        assert_eq!(finished.completion_start_us, Some(1_714_557_600_400_000));
    }

    #[test]
    fn a_patch_takes_its_id_from_the_path_and_errors_mark_the_run() {
        let batch = patch_to_batch(CHILD, &json!({"error": "Timeout", "end_time": "2024-05-01T10:00:05Z"})).unwrap();
        let o = &batch.observations[0];
        assert_eq!(o.id, CHILD);
        assert_eq!(o.trace_id, None); // resolved by the store from the stored row
        assert_eq!(o.level.as_deref(), Some("ERROR"));
        assert_eq!(o.status_message.as_deref(), Some("Timeout"));
    }

    #[test]
    fn multipart_runs_are_reassembled_from_their_parts() {
        let boundary = "XyZ";
        let mut root = root_run();
        let inputs = root.as_object_mut().unwrap().remove("inputs").unwrap();
        let part = |name: &str, body: &str| {
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}\r\n", body.len())
        };
        let body = [
            part(&format!("post.{TRACE}"), &root.to_string()),
            part(&format!("post.{TRACE}.inputs"), &inputs.to_string()),
            part(&format!("patch.{TRACE}"), &json!({"trace_id": TRACE, "end_time": "2024-05-01T10:00:02Z"}).to_string()),
            part(&format!("patch.{TRACE}.outputs"), &json!({"answer": "a tool"}).to_string()),
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"attachment.{TRACE}.photo\"; filename=\"p.png\"\r\nContent-Type: image/png\r\n\r\n\u{1}\u{2}\r\n"),
            part("feedback.f1", &json!({"id": "f1", "run_id": TRACE, "key": "correct", "score": true}).to_string()),
            format!("--{boundary}--\r\n"),
        ]
        .concat();
        let parts = parse_multipart(body.as_bytes(), boundary).unwrap();
        assert_eq!(parts.len(), 6);
        assert_eq!(super::boundary("multipart/form-data; boundary=\"XyZ\"").as_deref(), Some("XyZ"));
        assert_eq!(super::boundary("Multipart/Form-Data; Boundary=AbC-1").as_deref(), Some("AbC-1"));

        let batch = multipart_to_batch(&parts).unwrap();
        let post = &batch.observations[0];
        assert_eq!(post.input, Some(json!({"question": "What is DevsPro?"})));
        assert_eq!(post.metadata.as_ref().unwrap()["attachments_not_stored"], json!(["photo"]));
        let patch = &batch.observations[1];
        assert_eq!(patch.output, Some(json!({"answer": "a tool"})));
        assert_eq!(batch.traces[1].output, Some(json!({"answer": "a tool"})));
        assert_eq!(batch.scores[0].value, Some(1.0));
        assert_eq!(batch.scores[0].observation_id.as_deref(), Some(TRACE));
    }

    /// A list scanned for each part would take minutes here: 400,000 lookups
    /// among up to 200,000 runs.
    #[test]
    fn runs_are_found_by_id_however_many_there_are() {
        let mut runs = Runs::default();
        for round in 0..2 {
            for i in 0..200_000 {
                runs.entry(&format!("run-{i}")).insert(format!("field-{round}"), Value::Null);
            }
        }
        assert_eq!(runs.order.len(), 200_000);
        assert_eq!(runs.order[7].0, "run-7");
        assert_eq!(runs.order[7].1.len(), 2);
    }

    #[test]
    fn openai_wrapper_usage_is_found() {
        let run = json!({"id": "x", "run_type": "llm", "name": "ChatOpenAI",
            "inputs": {"model": "gpt-4.1", "messages": []},
            "outputs": {"choices": [], "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}}});
        let mut batch = Batch::default();
        run_to_batch(&run, true, &mut batch).unwrap();
        let o = &batch.observations[0];
        assert_eq!(o.total_tokens, Some(5));
        assert_eq!(o.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(o.trace_id.as_deref(), Some("x"));
    }
}
