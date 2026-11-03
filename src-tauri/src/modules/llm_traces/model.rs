//! The one shape every protocol is translated into before it is stored.
//!
//! Langfuse, LangSmith and OpenTelemetry describe the same thing three ways: a
//! trace (one request through an LLM app) made of nested observations (spans,
//! LLM calls, tool calls…). Each protocol module turns its wire format into a
//! [`Batch`] of upserts; `store` merges them, so a span's "create" and its later
//! "update" — or a child that arrives before its parent — end up as one row.
//!
//! Every field is optional on purpose: an update carries only what changed, and
//! the store keeps whatever a field already held when the update leaves it out.

use serde_json::Value;

/// Which protocol a trace arrived over. Stored as text; also what the page's
/// source filter offers.
pub const SOURCE_LANGFUSE: &str = "langfuse";
pub const SOURCE_LANGSMITH: &str = "langsmith";
pub const SOURCE_OTEL: &str = "otel";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TraceUpsert {
    pub id: String,
    pub source: &'static str,
    pub project: Option<String>,
    /// Set explicitly by the SDK: overwrites what the trace held.
    pub name: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    /// Taken from the root span when the SDK named nothing at trace level: only
    /// fills a field that is still empty, so an explicit value always wins
    /// whichever of the two arrives first.
    pub default_name: Option<String>,
    pub default_input: Option<Value>,
    pub default_output: Option<Value>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Merged key by key into what is stored, never replacing it wholesale.
    pub metadata: Option<Value>,
    pub release: Option<String>,
    pub version: Option<String>,
    pub environment: Option<String>,
    pub start_us: Option<i64>,
    pub end_us: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservationUpsert {
    pub id: String,
    /// `None` when the wire format left it out of an update; the store then
    /// keeps the row's, or takes the parent's, or starts a trace of its own.
    pub trace_id: Option<String>,
    pub parent_id: Option<String>,
    /// span, generation, event, tool, chain, retriever, embedding, agent… —
    /// lower case. Only `generation` and `embedding` count towards token totals,
    /// because wrappers often repeat their children's usage.
    pub kind: Option<String>,
    pub name: Option<String>,
    pub start_us: Option<i64>,
    pub end_us: Option<i64>,
    /// When the first token streamed back, for time-to-first-token.
    pub completion_start_us: Option<i64>,
    pub model: Option<String>,
    pub model_parameters: Option<Value>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub metadata: Option<Value>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    /// Only what the SDK reported; nothing is priced locally.
    pub cost: Option<f64>,
    /// DEBUG, DEFAULT, WARNING or ERROR, as Langfuse names them.
    pub level: Option<String>,
    pub status_message: Option<String>,
    /// Everything an OpenTelemetry span carried, untranslated, so nothing the
    /// mapping didn't know about is lost.
    pub attributes: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScoreUpsert {
    pub id: String,
    pub trace_id: Option<String>,
    pub observation_id: Option<String>,
    pub name: String,
    pub value: Option<f64>,
    pub string_value: Option<String>,
    pub comment: Option<String>,
    pub time_us: Option<i64>,
}

/// One request's worth of upserts, applied in a single transaction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    pub traces: Vec<TraceUpsert>,
    pub observations: Vec<ObservationUpsert>,
    pub scores: Vec<ScoreUpsert>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty() && self.observations.is_empty() && self.scores.is_empty()
    }
}

/// Token counts from whichever usage object an SDK sent. The names differ by
/// vendor and SDK generation, so this tries each spelling:
///
/// - Langfuse `usageDetails` / OTel `usage_details`: `input`, `output`, `total`
/// - Langfuse v2 `usage`: `promptTokens`, `completionTokens`, `totalTokens`
/// - OpenAI: `prompt_tokens`, `completion_tokens`, `total_tokens`
/// - LangChain `usage_metadata`, Anthropic: `input_tokens`, `output_tokens`
/// - Vercel AI SDK: `inputTokens`, `outputTokens`
pub fn usage_from(value: &Value) -> (Option<i64>, Option<i64>, Option<i64>) {
    let Some(obj) = value.as_object() else {
        return (None, None, None);
    };
    let pick = |keys: &[&str]| keys.iter().find_map(|k| obj.get(*k).and_then(as_i64));
    let input = pick(&["input", "input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]);
    let output = pick(&["output", "output_tokens", "outputTokens", "completion_tokens", "completionTokens"]);
    let total = pick(&["total", "total_tokens", "totalTokens"]);
    (input, output, total)
}

/// Total cost from a Langfuse `costDetails` object: its `total` when given,
/// otherwise the sum of the parts.
pub fn cost_from(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    let obj = value.as_object()?;
    if let Some(total) = obj.get("total").and_then(Value::as_f64) {
        return Some(total);
    }
    let parts: Vec<f64> = obj.values().filter_map(Value::as_f64).collect();
    (!parts.is_empty()).then(|| parts.iter().sum())
}

/// A number, or a string holding one — SDKs send both.
pub fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

pub fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

/// `null` and missing are the same thing to every SDK here.
pub fn present(value: Option<&Value>) -> Option<Value> {
    value.filter(|v| !v.is_null()).cloned()
}

/// A string that holds JSON is stored as that JSON, so `"{\"a\":1}"` shows as an
/// object. OpenTelemetry attributes can only be scalars, which is why every SDK
/// serialises inputs and outputs into a string before setting them.
pub fn json_or_string(text: &str) -> Value {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"') {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            return v;
        }
    }
    Value::String(text.to_string())
}

/// Microseconds since the epoch from whatever an SDK sent: an RFC 3339 string
/// (with or without a zone — LangSmith's Python SDK omits it and means UTC), or
/// a number whose magnitude says its unit (seconds, ms, µs or ns).
pub fn parse_time_us(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => {
            let f = n.as_f64()?;
            Some(scale_epoch(f))
        }
        Value::String(s) => parse_time_str(s),
        _ => None,
    }
}

fn scale_epoch(f: f64) -> i64 {
    let abs = f.abs();
    if abs < 1e11 {
        (f * 1e6) as i64 // seconds
    } else if abs < 1e14 {
        (f * 1e3) as i64 // milliseconds
    } else if abs < 1e17 {
        f as i64 // microseconds
    } else {
        (f / 1e3) as i64 // nanoseconds
    }
}

pub fn parse_time_str(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<f64>() {
        return Some(scale_epoch(n));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_micros());
    }
    // No zone: UTC. Both the `T` and the space separator turn up.
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, format) {
            return Some(naive.and_utc().timestamp_micros());
        }
    }
    None
}

pub fn now_us() -> i64 {
    chrono::Utc::now().timestamp_micros()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_reads_every_vendors_spelling() {
        assert_eq!(usage_from(&json!({"input": 3, "output": 4, "total": 7})), (Some(3), Some(4), Some(7)));
        assert_eq!(
            usage_from(&json!({"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12})),
            (Some(10), Some(2), Some(12))
        );
        assert_eq!(usage_from(&json!({"input_tokens": 5, "output_tokens": "6"})), (Some(5), Some(6), None));
        assert_eq!(usage_from(&json!({"promptTokens": 1, "completionTokens": 1})), (Some(1), Some(1), None));
        assert_eq!(usage_from(&json!("nope")), (None, None, None));
    }

    #[test]
    fn cost_prefers_total_and_sums_otherwise() {
        assert_eq!(cost_from(&json!({"input": 0.5, "output": 0.25})), Some(0.75));
        assert_eq!(cost_from(&json!({"input": 0.5, "total": 0.6})), Some(0.6));
        assert_eq!(cost_from(&json!({})), None);
    }

    #[test]
    fn times_parse_in_every_unit_and_format() {
        let expected = 1_700_000_000_123_000; // 2023-11-14T22:13:20.123Z
        assert_eq!(parse_time_str("2023-11-14T22:13:20.123Z"), Some(expected));
        assert_eq!(parse_time_str("2023-11-14T22:13:20.123000"), Some(expected));
        assert_eq!(parse_time_str("2023-11-14T23:13:20.123+01:00"), Some(expected));
        assert_eq!(parse_time_us(&json!(1_700_000_000_123i64)), Some(expected)); // ms
        assert_eq!(parse_time_us(&json!(1_700_000_000.123)), Some(expected)); // s
        assert_eq!(parse_time_us(&json!(1_700_000_000_123_000_000i64)), Some(expected)); // ns
        assert_eq!(parse_time_us(&json!("garbage")), None);
    }

    #[test]
    fn json_strings_become_json_and_plain_text_stays_text() {
        assert_eq!(json_or_string("{\"a\":1}"), json!({"a": 1}));
        assert_eq!(json_or_string("[1,2]"), json!([1, 2]));
        assert_eq!(json_or_string("hello"), json!("hello"));
        assert_eq!(json_or_string("{not json"), json!("{not json"));
    }
}
