//! Langfuse's batch ingestion API, `POST /api/public/ingestion`.
//!
//! The body is `{"batch": [{"id", "type", "timestamp", "body"}, …]}`; each
//! event's `type` is `<entity>-<create|update>` (`trace-create`,
//! `generation-update`, `score-create`, …) and its `body` the entity's fields in
//! camelCase. Langfuse answers 207 Multi-Status with a success or an error per
//! event, so one malformed event never fails the rest; this does the same.
//!
//! Langfuse's own SDKs moved to OpenTelemetry in Python v3 and JS v4 (see
//! `otel`), but Langfuse v2 SDKs, LiteLLM's callback and many integrations
//! still post here.

use super::model::{
    as_string, cost_from, parse_time_us, present, usage_from, Batch, ObservationUpsert, ScoreUpsert,
    TraceUpsert, SOURCE_LANGFUSE,
};
use serde_json::{json, Value};

pub struct Ingested {
    pub batch: Batch,
    /// The 207 body Langfuse's SDKs expect back.
    pub response: Value,
}

/// `project` is the public key the request authenticated with: in Langfuse a
/// key pair belongs to exactly one project, so traces are grouped by it here.
pub fn ingest(body: &Value, project: Option<&str>) -> Ingested {
    let mut batch = Batch::default();
    let mut successes = Vec::new();
    let mut errors = Vec::new();
    let events = body.get("batch").and_then(Value::as_array).cloned().unwrap_or_default();
    for event in &events {
        let event_id = event.get("id").and_then(as_string).unwrap_or_default();
        match apply_event(event, project, &mut batch) {
            Ok(()) => successes.push(json!({"id": event_id, "status": 201})),
            Err(message) => errors.push(json!({"id": event_id, "status": 400, "message": message, "error": message})),
        }
    }
    Ingested { batch, response: json!({"successes": successes, "errors": errors}) }
}

fn apply_event(event: &Value, project: Option<&str>, batch: &mut Batch) -> Result<(), String> {
    let kind = event.get("type").and_then(Value::as_str).ok_or("event has no type")?;
    let body = event.get("body").filter(|b| b.is_object()).ok_or("event has no body")?;
    let event_time = event.get("timestamp").and_then(parse_time_us);
    // "generation-create" → ("generation", "create"); "sdk-log" has no verb.
    let entity = kind.rsplit_once('-').map(|(entity, _)| entity).unwrap_or(kind);
    match entity {
        "sdk" => Ok(()), // sdk-log: the SDK's own diagnostics, not trace data
        "trace" => {
            batch.traces.push(trace(body, project, event_time)?);
            Ok(())
        }
        "score" => {
            batch.scores.push(score(body, event_time)?);
            Ok(())
        }
        _ => {
            let obs = observation(entity, body)?;
            // So the trace carries the project even when no trace-create came.
            if let Some(trace_id) = obs.trace_id.clone() {
                batch.traces.push(TraceUpsert {
                    id: trace_id,
                    source: SOURCE_LANGFUSE,
                    project: project.map(str::to_string),
                    ..Default::default()
                });
            }
            batch.observations.push(obs);
            Ok(())
        }
    }
}

fn text(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(as_string)
}

fn trace(body: &Value, project: Option<&str>, event_time: Option<i64>) -> Result<TraceUpsert, String> {
    let id = text(body, "id").ok_or("trace has no id")?;
    Ok(TraceUpsert {
        id,
        source: SOURCE_LANGFUSE,
        project: project.map(str::to_string),
        name: text(body, "name"),
        input: present(body.get("input")),
        output: present(body.get("output")),
        user_id: text(body, "userId"),
        session_id: text(body, "sessionId"),
        tags: body.get("tags").and_then(Value::as_array).map(|tags| tags.iter().filter_map(as_string).collect()),
        metadata: present(body.get("metadata")),
        release: text(body, "release"),
        version: text(body, "version"),
        environment: text(body, "environment"),
        start_us: body.get("timestamp").and_then(parse_time_us).or(event_time),
        ..Default::default()
    })
}

fn observation(entity: &str, body: &Value) -> Result<ObservationUpsert, String> {
    let id = text(body, "id").ok_or("observation has no id")?;
    // The generic "observation-create" carries its kind in the body.
    let kind = if entity == "observation" {
        text(body, "type").map(|t| t.to_lowercase())
    } else {
        Some(entity.to_string())
    };
    let start = body.get("startTime").and_then(parse_time_us);
    let end = body.get("endTime").and_then(parse_time_us);
    let mut obs = ObservationUpsert {
        id,
        trace_id: text(body, "traceId"),
        parent_id: text(body, "parentObservationId"),
        kind: kind.clone(),
        name: text(body, "name"),
        start_us: start,
        // An event is a point in time.
        end_us: if kind.as_deref() == Some("event") { end.or(start) } else { end },
        completion_start_us: body.get("completionStartTime").and_then(parse_time_us),
        model: text(body, "model"),
        model_parameters: present(body.get("modelParameters")),
        input: present(body.get("input")),
        output: present(body.get("output")),
        metadata: present(body.get("metadata")),
        level: text(body, "level").map(|l| l.to_uppercase()),
        status_message: text(body, "statusMessage"),
        ..Default::default()
    };
    // usageDetails is the current field; `usage` the v2 one, which may also
    // carry the cost.
    for key in ["usageDetails", "usage"] {
        if let Some(usage) = body.get(key).filter(|u| u.is_object()) {
            let (input, output, total) = usage_from(usage);
            obs.input_tokens = obs.input_tokens.or(input);
            obs.output_tokens = obs.output_tokens.or(output);
            obs.total_tokens = obs.total_tokens.or(total);
            if obs.cost.is_none() {
                obs.cost = usage.get("totalCost").and_then(Value::as_f64);
            }
        }
    }
    if let Some(cost) = body.get("costDetails").and_then(cost_from) {
        obs.cost = Some(cost);
    }
    Ok(obs)
}

fn score(body: &Value, event_time: Option<i64>) -> Result<ScoreUpsert, String> {
    let name = text(body, "name").ok_or("score has no name")?;
    let value = body.get("value");
    Ok(ScoreUpsert {
        id: text(body, "id").unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        trace_id: text(body, "traceId"),
        observation_id: text(body, "observationId"),
        name,
        value: value.and_then(Value::as_f64).or_else(|| value.and_then(Value::as_bool).map(|b| if b { 1.0 } else { 0.0 })),
        string_value: value.filter(|v| v.is_string()).and_then(as_string),
        comment: text(body, "comment"),
        time_us: body.get("timestamp").and_then(parse_time_us).or(event_time),
    })
}

/// `POST /api/public/scores`: one score object from older SDKs, a JSON array
/// of them from Python v5.
pub fn score_request(body: &Value) -> Result<Vec<ScoreUpsert>, String> {
    match body {
        Value::Array(items) => items.iter().map(|item| score(item, None)).collect(),
        single => Ok(vec![score(single, None)?]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v2_sdk_batch_becomes_a_trace_with_a_generation() {
        let body = json!({
            "batch": [
                {"id": "e1", "type": "trace-create", "timestamp": "2024-05-01T10:00:00.000Z",
                 "body": {"id": "t1", "name": "qa", "userId": "u1", "sessionId": "s1", "tags": ["prod"],
                          "input": {"question": "why?"}, "metadata": {"k": "v"}}},
                {"id": "e2", "type": "generation-create", "timestamp": "2024-05-01T10:00:00.100Z",
                 "body": {"id": "g1", "traceId": "t1", "name": "llm", "startTime": "2024-05-01T10:00:00.100Z",
                          "model": "gpt-4o", "modelParameters": {"temperature": 0},
                          "input": [{"role": "user", "content": "why?"}]}},
                {"id": "e3", "type": "generation-update", "timestamp": "2024-05-01T10:00:01.000Z",
                 "body": {"id": "g1", "traceId": "t1", "endTime": "2024-05-01T10:00:01.000Z", "output": "because",
                          "usage": {"promptTokens": 5, "completionTokens": 1, "totalTokens": 6, "totalCost": 0.01}}},
                {"id": "e4", "type": "score-create", "timestamp": "2024-05-01T10:00:02.000Z",
                 "body": {"id": "sc1", "traceId": "t1", "name": "helpful", "value": 1}},
                {"id": "e5", "type": "sdk-log", "timestamp": "2024-05-01T10:00:02.000Z", "body": {"log": "x"}},
                {"id": "e6", "type": "span-create", "timestamp": "2024-05-01T10:00:02.000Z", "body": {"name": "no id"}}
            ]
        });
        let out = ingest(&body, Some("pk-lf-local"));
        assert_eq!(out.response["successes"].as_array().unwrap().len(), 5);
        assert_eq!(out.response["errors"][0]["id"], "e6");

        let trace = &out.batch.traces[0];
        assert_eq!((trace.id.as_str(), trace.name.as_deref(), trace.user_id.as_deref()), ("t1", Some("qa"), Some("u1")));
        assert_eq!(trace.project.as_deref(), Some("pk-lf-local"));
        assert_eq!(trace.tags, Some(vec!["prod".to_string()]));

        let gens: Vec<_> = out.batch.observations.iter().filter(|o| o.id == "g1").collect();
        assert_eq!(gens.len(), 2);
        assert_eq!(gens[0].kind.as_deref(), Some("generation"));
        assert_eq!(gens[0].model.as_deref(), Some("gpt-4o"));
        assert_eq!((gens[1].input_tokens, gens[1].output_tokens, gens[1].total_tokens), (Some(5), Some(1), Some(6)));
        assert_eq!(gens[1].cost, Some(0.01));
        assert_eq!(out.batch.scores[0].value, Some(1.0));
    }

    #[test]
    fn newer_observation_types_and_the_generic_form_keep_their_kind() {
        let body = json!({"batch": [
            {"id": "a", "type": "agent-create", "body": {"id": "o1", "traceId": "t"}},
            {"id": "b", "type": "observation-create", "body": {"id": "o2", "traceId": "t", "type": "EVENT",
                "startTime": "2024-05-01T10:00:00Z"}},
            {"id": "c", "type": "generation-create", "body": {"id": "o3", "traceId": "t",
                "usageDetails": {"input": 7, "output": 8}, "costDetails": {"input": 0.1, "output": 0.2}}}
        ]});
        let out = ingest(&body, None);
        let kinds: Vec<_> = out.batch.observations.iter().map(|o| o.kind.clone().unwrap()).collect();
        assert_eq!(kinds, vec!["agent", "event", "generation"]);
        let event = &out.batch.observations[1];
        assert_eq!(event.end_us, event.start_us);
        let generation = &out.batch.observations[2];
        assert_eq!((generation.input_tokens, generation.output_tokens), (Some(7), Some(8)));
        assert!((generation.cost.unwrap() - 0.3).abs() < 1e-9);
    }
}
