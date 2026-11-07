//! OpenTelemetry trace export (OTLP over HTTP), in both its encodings:
//! protobuf (`application/x-protobuf`, all the Python exporters) and JSON
//! (`application/json`, the JavaScript `exporter-trace-otlp-http`).
//!
//! Two endpoints arrive here:
//!
//! - `/api/public/otel/v1/traces`: where the Langfuse SDKs (Python v3+, JS v4+)
//!   export, since they are built on OpenTelemetry. They describe the trace with
//!   `langfuse.*` attributes on each span.
//! - `/v1/traces`: the standard OTLP path, so anything instrumented with
//!   OpenTelemetry can export to DevsPro by pointing
//!   `OTEL_EXPORTER_OTLP_ENDPOINT` at it — OpenLLMetry, OpenInference, the
//!   Vercel AI SDK, Pydantic AI / Logfire and the GenAI semantic conventions.
//!
//! Every span becomes an observation. What kind of observation, its input,
//! output, model and token counts are read from whichever of those attribute
//! dialects the span speaks, in the order `first_of` lists them; the raw
//! attributes are kept beside the result, minus the inputs and outputs they
//! were read from, so nothing the mapping missed is lost.

use super::model::{
    as_i64, as_string, cost_from, json_or_string, parse_time_us, usage_from, Batch, ObservationUpsert, TraceUpsert,
};
use base64::Engine;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

/// The messages of `opentelemetry/proto/collector/trace/v1` that carry data
/// DevsPro reads. Field numbers are OTLP's; fields not declared here (links,
/// dropped counts, trace state, schema URLs) are skipped by the decoder.
pub mod proto {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ExportTraceServiceRequest {
        #[prost(message, repeated, tag = "1")]
        pub resource_spans: Vec<ResourceSpans>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ResourceSpans {
        #[prost(message, optional, tag = "1")]
        pub resource: Option<Resource>,
        #[prost(message, repeated, tag = "2")]
        pub scope_spans: Vec<ScopeSpans>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Resource {
        #[prost(message, repeated, tag = "1")]
        pub attributes: Vec<KeyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ScopeSpans {
        #[prost(message, optional, tag = "1")]
        pub scope: Option<InstrumentationScope>,
        #[prost(message, repeated, tag = "2")]
        pub spans: Vec<Span>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct InstrumentationScope {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, tag = "2")]
        pub version: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Span {
        #[prost(bytes = "vec", tag = "1")]
        pub trace_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub span_id: Vec<u8>,
        #[prost(bytes = "vec", tag = "4")]
        pub parent_span_id: Vec<u8>,
        #[prost(string, tag = "5")]
        pub name: String,
        #[prost(int32, tag = "6")]
        pub kind: i32,
        #[prost(fixed64, tag = "7")]
        pub start_time_unix_nano: u64,
        #[prost(fixed64, tag = "8")]
        pub end_time_unix_nano: u64,
        #[prost(message, repeated, tag = "9")]
        pub attributes: Vec<KeyValue>,
        #[prost(message, repeated, tag = "11")]
        pub events: Vec<Event>,
        #[prost(message, optional, tag = "15")]
        pub status: Option<Status>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Event {
        #[prost(fixed64, tag = "1")]
        pub time_unix_nano: u64,
        #[prost(string, tag = "2")]
        pub name: String,
        #[prost(message, repeated, tag = "3")]
        pub attributes: Vec<KeyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Status {
        #[prost(string, tag = "2")]
        pub message: String,
        #[prost(int32, tag = "3")]
        pub code: i32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KeyValue {
        #[prost(string, tag = "1")]
        pub key: String,
        #[prost(message, optional, tag = "2")]
        pub value: Option<AnyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct AnyValue {
        #[prost(oneof = "any_value::Value", tags = "1, 2, 3, 4, 5, 6, 7")]
        pub value: Option<any_value::Value>,
    }

    pub mod any_value {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Value {
            #[prost(string, tag = "1")]
            StringValue(String),
            #[prost(bool, tag = "2")]
            BoolValue(bool),
            #[prost(int64, tag = "3")]
            IntValue(i64),
            #[prost(double, tag = "4")]
            DoubleValue(f64),
            #[prost(message, tag = "5")]
            ArrayValue(super::ArrayValue),
            #[prost(message, tag = "6")]
            KvlistValue(super::KeyValueList),
            #[prost(bytes, tag = "7")]
            BytesValue(Vec<u8>),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ArrayValue {
        #[prost(message, repeated, tag = "1")]
        pub values: Vec<AnyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KeyValueList {
        #[prost(message, repeated, tag = "1")]
        pub values: Vec<KeyValue>,
    }
}

/// One span in a shape both encodings decode into.
#[derive(Debug, Clone, Default)]
pub struct SpanData {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_ns: u64,
    pub end_ns: u64,
    pub attributes: Map<String, Value>,
    pub events: Vec<EventData>,
    /// 0 unset, 1 ok, 2 error.
    pub status_code: i32,
    pub status_message: String,
    pub resource: Map<String, Value>,
    pub scope: String,
}

#[derive(Debug, Clone, Default)]
pub struct EventData {
    pub time_ns: u64,
    pub name: String,
    pub attributes: Map<String, Value>,
}

/// Which dialect owns the trace-level fields: a Langfuse export carries its
/// project in the request's credentials; any other exporter names its service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Langfuse,
    Generic,
}

// ---------------------------------------------------------------------------
// Decoding

pub fn decode_protobuf(body: &[u8]) -> Result<Vec<SpanData>, String> {
    use prost::Message;
    let request =
        proto::ExportTraceServiceRequest::decode(body).map_err(|e| format!("not an OTLP protobuf trace export: {e}"))?;
    let mut spans = Vec::new();
    for resource_spans in request.resource_spans {
        let resource = resource_spans.resource.map(|r| kv_map(r.attributes)).unwrap_or_default();
        for scope_spans in resource_spans.scope_spans {
            let scope = scope_spans.scope.map(|s| s.name).unwrap_or_default();
            for span in scope_spans.spans {
                let status = span.status.unwrap_or_default();
                spans.push(SpanData {
                    trace_id: hex(&span.trace_id),
                    span_id: hex(&span.span_id),
                    parent_span_id: (!span.parent_span_id.is_empty()).then(|| hex(&span.parent_span_id)),
                    name: span.name,
                    start_ns: span.start_time_unix_nano,
                    end_ns: span.end_time_unix_nano,
                    attributes: kv_map(span.attributes),
                    events: span
                        .events
                        .into_iter()
                        .map(|e| EventData { time_ns: e.time_unix_nano, name: e.name, attributes: kv_map(e.attributes) })
                        .collect(),
                    status_code: status.code,
                    status_message: status.message,
                    resource: resource.clone(),
                    scope: scope.clone(),
                });
            }
        }
    }
    Ok(spans)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn kv_map(kvs: Vec<proto::KeyValue>) -> Map<String, Value> {
    kvs.into_iter().map(|kv| (kv.key, kv.value.map(any_value).unwrap_or(Value::Null))).collect()
}

fn any_value(v: proto::AnyValue) -> Value {
    use proto::any_value::Value as V;
    match v.value {
        None => Value::Null,
        Some(V::StringValue(s)) => Value::String(s),
        Some(V::BoolValue(b)) => Value::Bool(b),
        Some(V::IntValue(i)) => Value::from(i),
        Some(V::DoubleValue(d)) => serde_json::Number::from_f64(d).map(Value::Number).unwrap_or(Value::Null),
        Some(V::ArrayValue(a)) => Value::Array(a.values.into_iter().map(any_value).collect()),
        Some(V::KvlistValue(kv)) => Value::Object(kv_map(kv.values)),
        Some(V::BytesValue(b)) => Value::String(base64::engine::general_purpose::STANDARD.encode(b)),
    }
}

/// OTLP/JSON. Its rules differ from protobuf's canonical JSON in ways
/// exporters rely on: trace and span ids are hex (not base64), 64-bit numbers
/// may be strings, enum values may be integers or names, and keys are
/// lowerCamelCase — though snake_case is accepted too, as the spec asks.
pub fn decode_json(body: &Value) -> Result<Vec<SpanData>, String> {
    let resource_spans = field(body, "resourceSpans", "resource_spans")
        .and_then(Value::as_array)
        .ok_or("not an OTLP JSON trace export: no resourceSpans")?;
    let mut spans = Vec::new();
    for rs in resource_spans {
        let resource = field(rs, "resource", "resource").map(json_attributes).unwrap_or_default();
        for ss in field(rs, "scopeSpans", "scope_spans").and_then(Value::as_array).into_iter().flatten() {
            let scope = ss
                .get("scope")
                .and_then(|s| s.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            for span in ss.get("spans").and_then(Value::as_array).into_iter().flatten() {
                let status = span.get("status");
                spans.push(SpanData {
                    trace_id: json_id(field(span, "traceId", "trace_id")),
                    span_id: json_id(field(span, "spanId", "span_id")),
                    parent_span_id: Some(json_id(field(span, "parentSpanId", "parent_span_id"))).filter(|s| !s.is_empty()),
                    name: span.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                    start_ns: json_u64(field(span, "startTimeUnixNano", "start_time_unix_nano")),
                    end_ns: json_u64(field(span, "endTimeUnixNano", "end_time_unix_nano")),
                    attributes: json_attributes(span),
                    events: span
                        .get("events")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|e| EventData {
                            time_ns: json_u64(field(e, "timeUnixNano", "time_unix_nano")),
                            name: e.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            attributes: json_attributes(e),
                        })
                        .collect(),
                    status_code: status.and_then(|s| s.get("code")).map(status_code).unwrap_or(0),
                    status_message: status
                        .and_then(|s| s.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    resource: resource.clone(),
                    scope: scope.clone(),
                });
            }
        }
    }
    Ok(spans)
}

fn field<'a>(v: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    v.get(camel).or_else(|| v.get(snake))
}

/// Hex as the spec says; base64 from exporters that followed protobuf's JSON
/// mapping instead, converted so both spellings name the same span.
fn json_id(v: Option<&Value>) -> String {
    let Some(s) = v.and_then(Value::as_str) else { return String::new() };
    if s.chars().all(|c| c.is_ascii_hexdigit()) {
        return s.to_ascii_lowercase();
    }
    base64::engine::general_purpose::STANDARD.decode(s).map(|b| hex(&b)).unwrap_or_else(|_| s.to_string())
}

fn json_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)).unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn status_code(v: &Value) -> i32 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or(0) as i32,
        Value::String(s) if s.ends_with("ERROR") => 2,
        Value::String(s) if s.ends_with("OK") => 1,
        _ => 0,
    }
}

fn json_attributes(holder: &Value) -> Map<String, Value> {
    holder
        .get("attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|kv| Some((kv.get("key")?.as_str()?.to_string(), kv.get("value").map(json_any).unwrap_or(Value::Null))))
        .collect()
}

fn json_any(v: &Value) -> Value {
    let Some(obj) = v.as_object() else { return v.clone() };
    if let Some(s) = field(v, "stringValue", "string_value") {
        return s.clone();
    }
    if let Some(b) = field(v, "boolValue", "bool_value") {
        return b.clone();
    }
    if let Some(i) = field(v, "intValue", "int_value") {
        return as_i64(i).map(Value::from).unwrap_or(Value::Null);
    }
    if let Some(d) = field(v, "doubleValue", "double_value") {
        return match d {
            Value::String(s) => s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null),
            other => other.clone(),
        };
    }
    if let Some(a) = field(v, "arrayValue", "array_value") {
        return Value::Array(a.get("values").and_then(Value::as_array).into_iter().flatten().map(json_any).collect());
    }
    if let Some(kv) = field(v, "kvlistValue", "kvlist_value") {
        return Value::Object(json_attributes(&serde_json::json!({ "attributes": kv.get("values").cloned().unwrap_or(Value::Null) })));
    }
    if let Some(b) = field(v, "bytesValue", "bytes_value") {
        return b.clone();
    }
    if obj.is_empty() {
        return Value::Null;
    }
    v.clone()
}

// ---------------------------------------------------------------------------
// Mapping

/// Turns decoded spans into upserts. `project` is the Langfuse public key for
/// `Flavor::Langfuse`; a generic export is grouped by its `service.name`.
pub fn to_batch(spans: Vec<SpanData>, flavor: Flavor, project: Option<&str>) -> Batch {
    let source = match flavor {
        Flavor::Langfuse => super::model::SOURCE_LANGFUSE,
        Flavor::Generic => super::model::SOURCE_OTEL,
    };
    let mut batch = Batch::default();
    for span in spans {
        if span.trace_id.is_empty() || span.span_id.is_empty() {
            continue;
        }
        let project = match flavor {
            Flavor::Langfuse => project.map(str::to_string),
            Flavor::Generic => span.resource.get("service.name").and_then(as_string),
        };
        let (obs, trace) = map_span(span, source, project);
        batch.traces.push(trace);
        batch.observations.push(obs);
    }
    batch
}

struct Attrs<'a> {
    map: &'a Map<String, Value>,
    /// Keys an input or output was read from, left out of the raw copy.
    consumed: HashSet<String>,
}

impl<'a> Attrs<'a> {
    fn get(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key).filter(|v| !v.is_null())
    }

    fn str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(as_string)
    }

    fn first_str(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| self.str(k))
    }

    fn first_i64(&self, keys: &[&str]) -> Option<i64> {
        keys.iter().find_map(|k| self.get(k).and_then(as_i64))
    }

    /// A payload attribute: JSON strings are parsed, and the key is consumed.
    fn payload(&mut self, key: &str) -> Option<Value> {
        let value = self.get(key)?;
        self.consumed.insert(key.to_string());
        Some(match value {
            Value::String(s) => json_or_string(s),
            other => other.clone(),
        })
    }

    fn first_payload(&mut self, keys: &[&str]) -> Option<Value> {
        keys.iter().find_map(|k| self.payload(k))
    }

    /// `prefix.<key>` attributes gathered into one object, so metadata a
    /// Langfuse SDK flattened into `langfuse.observation.metadata.user_tier`
    /// reads as `{"user_tier": …}` again.
    fn prefixed_object(&self, prefix: &str) -> Option<Map<String, Value>> {
        let dotted = format!("{prefix}.");
        let found: Map<String, Value> = self
            .map
            .iter()
            .filter_map(|(k, v)| {
                let key = k.strip_prefix(&dotted)?;
                Some((key.to_string(), if let Value::String(s) = v { json_or_string(s) } else { v.clone() }))
            })
            .collect();
        (!found.is_empty()).then_some(found)
    }

    /// Chat messages spread over indexed attributes — OpenLLMetry's
    /// `gen_ai.prompt.0.role` / `gen_ai.prompt.0.content`, OpenInference's
    /// `llm.input_messages.0.message.role` — put back together in order.
    fn indexed_messages(&mut self, prefix: &str, strip: &str) -> Option<Value> {
        let dotted = format!("{prefix}.");
        let mut messages: BTreeMap<u32, Map<String, Value>> = BTreeMap::new();
        let mut used = Vec::new();
        for (key, value) in self.map.iter() {
            let Some(rest) = key.strip_prefix(&dotted) else { continue };
            let Some((index, field)) = rest.split_once('.') else { continue };
            let Ok(index) = index.parse::<u32>() else { continue };
            let field = field.strip_prefix(strip).unwrap_or(field);
            let value = if let Value::String(s) = value { json_or_string(s) } else { value.clone() };
            messages.entry(index).or_default().insert(field.to_string(), value);
            used.push(key.clone());
        }
        if messages.is_empty() {
            return None;
        }
        self.consumed.extend(used);
        Some(Value::Array(messages.into_values().map(Value::Object).collect()))
    }
}

fn ns_to_us(ns: u64) -> Option<i64> {
    (ns > 0).then(|| (ns / 1000) as i64)
}

fn map_span(span: SpanData, source: &'static str, project: Option<String>) -> (ObservationUpsert, TraceUpsert) {
    let mut a = Attrs { map: &span.attributes, consumed: HashSet::new() };
    let start_us = ns_to_us(span.start_ns);
    let end_us = ns_to_us(span.end_ns);

    let kind = observation_kind(&a, &span.name);

    let input = a
        .first_payload(&[
            "langfuse.observation.input",
            "input.value",
            "traceloop.entity.input",
            "gen_ai.input.messages",
            "gen_ai.tool.call.arguments",
        ])
        .or_else(|| a.indexed_messages("gen_ai.prompt", ""))
        .or_else(|| a.indexed_messages("llm.input_messages", "message."))
        .or_else(|| a.first_payload(&["gen_ai.prompt", "ai.prompt.messages", "ai.prompt", "ai.value", "ai.values"]))
        .or_else(|| messages_from_events(&span.events, false));
    let output = a
        .first_payload(&[
            "langfuse.observation.output",
            "output.value",
            "traceloop.entity.output",
            "gen_ai.output.messages",
            "gen_ai.tool.call.result",
        ])
        .or_else(|| a.indexed_messages("gen_ai.completion", ""))
        .or_else(|| a.indexed_messages("llm.output_messages", "message."))
        .or_else(|| {
            a.first_payload(&[
                "gen_ai.completion",
                "ai.response.text",
                "ai.response.object",
                "ai.response.toolCalls",
                "ai.result.text",
                "ai.embedding",
                "ai.embeddings",
            ])
        })
        .or_else(|| messages_from_events(&span.events, true));

    let model = a.first_str(&[
        "langfuse.observation.model.name",
        "gen_ai.response.model",
        "ai.model.id",
        "gen_ai.request.model",
        "llm.response.model",
        "llm.model_name",
    ]);
    let model_parameters = a
        .first_payload(&["langfuse.observation.model.parameters", "llm.invocation_parameters"])
        .or_else(|| gen_ai_parameters(&a));

    let usage = a.first_payload(&["langfuse.observation.usage_details"]);
    let (mut input_tokens, mut output_tokens, mut total_tokens) = usage.as_ref().map(usage_from).unwrap_or_default();
    input_tokens = input_tokens.or_else(|| {
        a.first_i64(&[
            "gen_ai.usage.input_tokens",
            "gen_ai.usage.prompt_tokens",
            "llm.token_count.prompt",
            "ai.usage.inputTokens",
            "ai.usage.promptTokens",
        ])
    });
    output_tokens = output_tokens.or_else(|| {
        a.first_i64(&[
            "gen_ai.usage.output_tokens",
            "gen_ai.usage.completion_tokens",
            "llm.token_count.completion",
            "ai.usage.outputTokens",
            "ai.usage.completionTokens",
        ])
    });
    total_tokens = total_tokens.or_else(|| a.first_i64(&["llm.usage.total_tokens", "llm.token_count.total", "gen_ai.usage.total_tokens"]));
    let cost = a.first_payload(&["langfuse.observation.cost_details"]).as_ref().and_then(cost_from);

    let completion_start_us = a
        .str("langfuse.observation.completion_start_time")
        .and_then(|s| parse_time_us(&json_or_string(&s)))
        .or_else(|| {
            let ms = a.get("ai.response.msToFirstChunk").and_then(Value::as_f64)?;
            Some(start_us? + (ms * 1000.0) as i64)
        });

    let exception = span.events.iter().find(|e| e.name == "exception").map(|e| {
        let kind = e.attributes.get("exception.type").and_then(as_string);
        let message = e.attributes.get("exception.message").and_then(as_string);
        match (kind, message) {
            (Some(k), Some(m)) => format!("{k}: {m}"),
            (k, m) => k.or(m).unwrap_or_else(|| "exception".to_string()),
        }
    });
    let level = a
        .str("langfuse.observation.level")
        .map(|l| l.to_uppercase())
        .or_else(|| (span.status_code == 2).then(|| "ERROR".to_string()));
    let status_message = a
        .str("langfuse.observation.status_message")
        .or_else(|| (!span.status_message.is_empty()).then(|| span.status_message.clone()))
        .or(exception);

    let mut metadata = a.payload("langfuse.observation.metadata").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    for prefix in ["langfuse.observation.metadata", "ai.telemetry.metadata"] {
        if let Some(found) = a.prefixed_object(prefix) {
            metadata.extend(found);
        }
    }
    if let Some(Value::Object(extra)) = a.payload("metadata") {
        metadata.extend(extra);
    }

    let is_root = span.parent_span_id.is_none() || a.get("langfuse.internal.as_root").and_then(Value::as_bool) == Some(true);
    let mut trace = TraceUpsert {
        id: span.trace_id.clone(),
        source,
        project,
        name: a.str("langfuse.trace.name"),
        input: a.payload("langfuse.trace.input"),
        output: a.payload("langfuse.trace.output"),
        user_id: a.first_str(&["user.id", "langfuse.user.id", "ai.telemetry.metadata.userId", "enduser.id"]),
        session_id: a.first_str(&[
            "session.id",
            "langfuse.session.id",
            "ai.telemetry.metadata.sessionId",
            "gen_ai.conversation.id",
        ]),
        tags: tags(&a),
        release: a.str("langfuse.release").or_else(|| span.resource.get("service.version").and_then(as_string)),
        version: a.str("langfuse.version"),
        environment: a.str("langfuse.environment").or_else(|| {
            ["deployment.environment.name", "deployment.environment"]
                .iter()
                .find_map(|k| span.resource.get(*k).and_then(as_string))
        }),
        start_us,
        end_us: end_us.or(start_us),
        ..Default::default()
    };
    let mut trace_metadata = a.payload("langfuse.trace.metadata").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    if let Some(found) = a.prefixed_object("langfuse.trace.metadata") {
        trace_metadata.extend(found);
    }
    if !trace_metadata.is_empty() {
        trace.metadata = Some(Value::Object(trace_metadata));
    }
    if is_root {
        trace.default_name = Some(span.name.clone());
        trace.default_input = input.clone();
        trace.default_output = output.clone();
    }

    let consumed = a.consumed;
    let raw: Map<String, Value> = span
        .attributes
        .iter()
        .filter(|(k, _)| !consumed.contains(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut attributes = Map::new();
    attributes.insert("span".into(), Value::Object(raw));
    if !span.resource.is_empty() {
        attributes.insert("resource".into(), Value::Object(span.resource.clone()));
    }
    if !span.scope.is_empty() {
        attributes.insert("scope".into(), Value::String(span.scope.clone()));
    }
    if !span.events.is_empty() {
        attributes.insert(
            "events".into(),
            Value::Array(
                span.events
                    .iter()
                    .map(|e| serde_json::json!({"name": e.name, "time_us": ns_to_us(e.time_ns), "attributes": e.attributes}))
                    .collect(),
            ),
        );
    }

    let obs = ObservationUpsert {
        id: span.span_id,
        trace_id: Some(span.trace_id),
        parent_id: span.parent_span_id,
        kind: Some(kind),
        name: Some(span.name),
        start_us,
        end_us: if end_us.is_some() { end_us } else { start_us },
        completion_start_us,
        model,
        model_parameters,
        input,
        output,
        metadata: (!metadata.is_empty()).then_some(Value::Object(metadata)),
        input_tokens,
        output_tokens,
        total_tokens,
        cost,
        level,
        status_message,
        attributes: Some(Value::Object(attributes)),
    };
    (obs, trace)
}

/// Langfuse's own type when the span has one, then OpenInference's and
/// OpenLLMetry's span kinds, then what the GenAI conventions imply. Anything
/// that names a model it called counts as a generation.
fn observation_kind(a: &Attrs, name: &str) -> String {
    if let Some(kind) = a.str("langfuse.observation.type") {
        return kind.to_lowercase();
    }
    if let Some(kind) = a.str("openinference.span.kind") {
        return match kind.to_uppercase().as_str() {
            "LLM" => "generation".to_string(),
            other => other.to_lowercase(),
        };
    }
    if let Some(operation) = a.str("gen_ai.operation.name") {
        match operation.as_str() {
            "chat" | "text_completion" | "generate_content" | "completion" | "generate" => return "generation".into(),
            "embeddings" | "embedding" => return "embedding".into(),
            "execute_tool" => return "tool".into(),
            "invoke_agent" | "create_agent" => return "agent".into(),
            _ => {}
        }
    }
    if let Some(kind) = a.str("traceloop.span.kind") {
        return kind.to_lowercase();
    }
    if a.str("llm.request.type").as_deref() == Some("embedding") || name.ends_with(".doEmbed") {
        return "embedding".into();
    }
    let names_a_model = ["gen_ai.request.model", "gen_ai.response.model", "gen_ai.system", "llm.request.type"]
        .iter()
        .any(|k| a.get(k).is_some());
    if names_a_model || name.ends_with(".doGenerate") || name.ends_with(".doStream") {
        return "generation".into();
    }
    if a.get("gen_ai.tool.name").is_some() || a.get("ai.toolCall.name").is_some() {
        return "tool".into();
    }
    "span".into()
}

fn gen_ai_parameters(a: &Attrs) -> Option<Value> {
    let params: Map<String, Value> = a
        .map
        .iter()
        .filter_map(|(k, v)| {
            let name = k.strip_prefix("gen_ai.request.")?;
            (name != "model").then(|| (name.to_string(), v.clone()))
        })
        .collect();
    (!params.is_empty()).then_some(Value::Object(params))
}

fn tags(a: &Attrs) -> Option<Vec<String>> {
    for key in ["langfuse.trace.tags", "tag.tags"] {
        match a.get(key) {
            Some(Value::Array(items)) => return Some(items.iter().filter_map(as_string).collect()),
            Some(Value::String(s)) => {
                return Some(match json_or_string(s) {
                    Value::Array(items) => items.iter().filter_map(as_string).collect(),
                    _ => s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
                })
            }
            _ => {}
        }
    }
    None
}

/// The older GenAI convention put each message in a span event —
/// `gen_ai.user.message`, `gen_ai.choice` — rather than an attribute.
fn messages_from_events(events: &[EventData], output: bool) -> Option<Value> {
    let messages: Vec<Value> = events
        .iter()
        .filter_map(|e| {
            let role = if output {
                (e.name == "gen_ai.choice").then_some("assistant")?
            } else {
                e.name.strip_prefix("gen_ai.")?.strip_suffix(".message")?
            };
            let mut message = Map::new();
            message.insert("role".into(), Value::String(role.to_string()));
            for (k, v) in &e.attributes {
                let key = k.strip_prefix("gen_ai.event.").unwrap_or(k);
                message.insert(key.to_string(), if let Value::String(s) = v { json_or_string(s) } else { v.clone() });
            }
            Some(Value::Object(message))
        })
        .collect();
    (!messages.is_empty()).then_some(Value::Array(messages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use serde_json::json;

    fn kv(key: &str, value: &str) -> proto::KeyValue {
        proto::KeyValue {
            key: key.into(),
            value: Some(proto::AnyValue { value: Some(proto::any_value::Value::StringValue(value.into())) }),
        }
    }

    fn kv_int(key: &str, value: i64) -> proto::KeyValue {
        proto::KeyValue { key: key.into(), value: Some(proto::AnyValue { value: Some(proto::any_value::Value::IntValue(value)) }) }
    }

    /// A Langfuse Python SDK v3 export, as its protobuf exporter sends it.
    fn langfuse_export() -> Vec<u8> {
        let trace_id = vec![0xab; 16];
        let root = proto::Span {
            trace_id: trace_id.clone(),
            span_id: vec![1; 8],
            name: "handle-request".into(),
            start_time_unix_nano: 1_700_000_000_000_000_000,
            end_time_unix_nano: 1_700_000_002_000_000_000,
            attributes: vec![
                kv("langfuse.observation.type", "span"),
                kv("langfuse.trace.name", "support-bot"),
                kv("user.id", "user-1"),
                kv("session.id", "sess-1"),
                kv("langfuse.observation.input", "{\"question\":\"refund?\"}"),
                kv("langfuse.observation.output", "\"Yes\""),
                kv("langfuse.observation.metadata.tier", "\"gold\""),
                kv("langfuse.environment", "dev"),
            ],
            ..Default::default()
        };
        let generation = proto::Span {
            trace_id,
            span_id: vec![2; 8],
            parent_span_id: vec![1; 8],
            name: "openai-call".into(),
            start_time_unix_nano: 1_700_000_000_500_000_000,
            end_time_unix_nano: 1_700_000_001_500_000_000,
            attributes: vec![
                kv("langfuse.observation.type", "generation"),
                kv("langfuse.observation.model.name", "gpt-4o-mini"),
                kv("langfuse.observation.model.parameters", "{\"temperature\":0.2}"),
                kv("langfuse.observation.usage_details", "{\"input\":12,\"output\":3}"),
                kv("langfuse.observation.cost_details", "{\"total\":0.0004}"),
                kv("langfuse.observation.input", "[{\"role\":\"user\",\"content\":\"refund?\"}]"),
                kv("langfuse.observation.completion_start_time", "\"2023-11-14T22:13:20.700Z\""),
            ],
            status: Some(proto::Status { code: 2, message: "rate limited".into() }),
            ..Default::default()
        };
        proto::ExportTraceServiceRequest {
            resource_spans: vec![proto::ResourceSpans {
                resource: Some(proto::Resource { attributes: vec![kv("service.name", "bot")] }),
                scope_spans: vec![proto::ScopeSpans {
                    scope: Some(proto::InstrumentationScope { name: "langfuse-sdk".into(), version: "3.0".into() }),
                    spans: vec![root, generation],
                }],
            }],
        }
        .encode_to_vec()
    }

    #[test]
    fn a_langfuse_v3_protobuf_export_maps_to_a_trace_and_its_observations() {
        let spans = decode_protobuf(&langfuse_export()).unwrap();
        let batch = to_batch(spans, Flavor::Langfuse, Some("pk-lf-dev"));
        assert_eq!(batch.observations.len(), 2);

        let root = &batch.observations[0];
        assert_eq!(root.id, "0101010101010101");
        assert_eq!(root.trace_id.as_deref(), Some("abababababababababababababababab"));
        assert_eq!(root.input, Some(json!({"question": "refund?"})));
        assert_eq!(root.output, Some(json!("Yes")));
        assert_eq!(root.metadata, Some(json!({"tier": "gold"})));
        assert_eq!(root.start_us, Some(1_700_000_000_000_000));

        let trace = &batch.traces[0];
        assert_eq!(trace.name.as_deref(), Some("support-bot"));
        assert_eq!(trace.default_name.as_deref(), Some("handle-request"));
        assert_eq!(trace.default_input, Some(json!({"question": "refund?"})));
        assert_eq!((trace.user_id.as_deref(), trace.session_id.as_deref()), (Some("user-1"), Some("sess-1")));
        assert_eq!(trace.project.as_deref(), Some("pk-lf-dev"));
        assert_eq!(trace.environment.as_deref(), Some("dev"));

        let generation = &batch.observations[1];
        assert_eq!(generation.kind.as_deref(), Some("generation"));
        assert_eq!(generation.parent_id.as_deref(), Some("0101010101010101"));
        assert_eq!(generation.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(generation.model_parameters, Some(json!({"temperature": 0.2})));
        assert_eq!((generation.input_tokens, generation.output_tokens), (Some(12), Some(3)));
        assert_eq!(generation.cost, Some(0.0004));
        assert_eq!(generation.completion_start_us, Some(1_700_000_000_700_000));
        assert_eq!(generation.level.as_deref(), Some("ERROR"));
        assert_eq!(generation.status_message.as_deref(), Some("rate limited"));
        // A child span names no trace-level defaults.
        assert_eq!(batch.traces[1].default_name, None);
        // Inputs aren't stored twice: the raw attribute copy drops them.
        let raw = &generation.attributes.as_ref().unwrap()["span"];
        assert!(raw.get("langfuse.observation.input").is_none());
        assert!(raw.get("langfuse.observation.model.name").is_some());
    }

    #[test]
    fn otlp_json_with_genai_conventions_and_indexed_prompts() {
        let body = json!({"resourceSpans": [{
            "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "rag-api"}}]},
            "scopeSpans": [{"scope": {"name": "opentelemetry.instrumentation.openai"}, "spans": [{
                "traceId": "5B8EFFF798038103D269B633813FC60C",
                "spanId": "EEE19B7EC3C1B174",
                "name": "openai.chat",
                "kind": 3,
                "startTimeUnixNano": "1700000000000000000",
                "endTimeUnixNano": 1700000001000000000u64,
                "attributes": [
                    {"key": "gen_ai.system", "value": {"stringValue": "openai"}},
                    {"key": "gen_ai.request.model", "value": {"stringValue": "gpt-4o"}},
                    {"key": "gen_ai.request.temperature", "value": {"doubleValue": 0.5}},
                    {"key": "gen_ai.usage.input_tokens", "value": {"intValue": "20"}},
                    {"key": "gen_ai.usage.output_tokens", "value": {"intValue": 5}},
                    {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "system"}},
                    {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "Be brief."}},
                    {"key": "gen_ai.prompt.1.role", "value": {"stringValue": "user"}},
                    {"key": "gen_ai.prompt.1.content", "value": {"stringValue": "Hi"}},
                    {"key": "gen_ai.completion.0.content", "value": {"stringValue": "Hello!"}},
                    {"key": "tags", "value": {"arrayValue": {"values": [{"stringValue": "a"}]}}}
                ],
                "status": {"code": "STATUS_CODE_OK"}
            }]}]
        }]});
        let spans = decode_json(&body).unwrap();
        assert_eq!(spans[0].trace_id, "5b8efff798038103d269b633813fc60c");
        assert_eq!(spans[0].attributes["tags"], json!(["a"]));
        let batch = to_batch(spans, Flavor::Generic, None);
        let o = &batch.observations[0];
        assert_eq!(o.kind.as_deref(), Some("generation"));
        assert_eq!(o.model.as_deref(), Some("gpt-4o"));
        assert_eq!((o.input_tokens, o.output_tokens), (Some(20), Some(5)));
        assert_eq!(o.model_parameters, Some(json!({"temperature": 0.5})));
        assert_eq!(
            o.input,
            Some(json!([{"role": "system", "content": "Be brief."}, {"role": "user", "content": "Hi"}]))
        );
        assert_eq!(o.output, Some(json!([{"content": "Hello!"}])));
        assert_eq!((o.start_us, o.end_us), (Some(1_700_000_000_000_000), Some(1_700_000_001_000_000)));
        assert_eq!(batch.traces[0].project.as_deref(), Some("rag-api"));
        assert_eq!(batch.traces[0].default_name.as_deref(), Some("openai.chat"));
    }

    #[test]
    fn openinference_and_vercel_spans_are_classified() {
        let span = |attrs: Vec<proto::KeyValue>, name: &str| SpanData {
            trace_id: "t".into(),
            span_id: name.into(),
            name: name.into(),
            attributes: kv_map(attrs),
            ..Default::default()
        };
        let spans = vec![
            span(
                vec![
                    kv("openinference.span.kind", "LLM"),
                    kv("llm.model_name", "claude"),
                    kv_int("llm.token_count.prompt", 9),
                    kv("llm.input_messages.0.message.role", "user"),
                    kv("llm.input_messages.0.message.content", "q"),
                    kv("output.value", "a"),
                ],
                "llm",
            ),
            span(vec![kv("openinference.span.kind", "RETRIEVER"), kv("input.value", "query")], "retrieve"),
            span(vec![kv("ai.model.id", "gpt"), kv_int("ai.usage.promptTokens", 4)], "ai.generateText"),
            span(vec![kv("gen_ai.system", "openai"), kv_int("ai.usage.promptTokens", 4)], "ai.generateText.doGenerate"),
            span(vec![kv("traceloop.span.kind", "workflow"), kv("traceloop.entity.input", "{\"x\":1}")], "wf"),
        ];
        let batch = to_batch(spans, Flavor::Generic, None);
        let kinds: Vec<_> = batch.observations.iter().map(|o| o.kind.clone().unwrap()).collect();
        assert_eq!(kinds, vec!["generation", "retriever", "span", "generation", "workflow"]);
        let llm = &batch.observations[0];
        assert_eq!(llm.model.as_deref(), Some("claude"));
        assert_eq!(llm.input_tokens, Some(9));
        assert_eq!(llm.input, Some(json!([{"role": "user", "content": "q"}])));
        assert_eq!(llm.output, Some(json!("a")));
        assert_eq!(batch.observations[4].input, Some(json!({"x": 1})));
    }

    #[test]
    fn exceptions_name_the_error_and_garbage_is_refused() {
        let span = SpanData {
            trace_id: "t".into(),
            span_id: "s".into(),
            status_code: 2,
            events: vec![EventData {
                name: "exception".into(),
                attributes: kv_map(vec![kv("exception.type", "ValueError"), kv("exception.message", "bad")]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let batch = to_batch(vec![span], Flavor::Generic, None);
        assert_eq!(batch.observations[0].status_message.as_deref(), Some("ValueError: bad"));
        assert_eq!(batch.observations[0].level.as_deref(), Some("ERROR"));
        assert!(decode_json(&json!({"hello": 1})).is_err());
        assert!(decode_protobuf(b"\xff\xff\xff").is_err());
    }
}
