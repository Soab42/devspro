# LLM Traces (beta)

A local stand-in for [Langfuse](https://github.com/langfuse/langfuse) and
LangSmith, in the way the Mail Catcher stands in for SMTP. You point an LLM
app's existing SDK at DevsPro, and every trace lands on this machine. The trace
holds the prompts, responses, tool calls, tokens, timings and errors, and you
read it in the app. There is no account and no cloud, and nothing leaves the
laptop.

This document covers the research behind the module, what the beta does, how
it was verified, and what comes next.

## Why a collector, not a client

There were three ways to read "a Langfuse / LangSmith connector for local":

| Option | What it is | Verdict |
| --- | --- | --- |
| **A. Built-in collector** | DevsPro speaks the SDKs' ingestion protocols itself and stores what arrives | **Built (this beta)** |
| B. Run real Langfuse in Docker | `docker compose` with langfuse-web, langfuse-worker, Postgres, ClickHouse, Redis and MinIO | Later, optional: it's about 4 GB of RAM and six containers to view traces |
| C. Remote-account browser | A viewer for traces already in Langfuse Cloud or LangSmith | Later, optional: it needs credentials and a network, which is the opposite of "local" |

Option A is what the Mail Catcher does for mail. The app changes nothing in its
code, only an environment variable, and DevsPro catches what it would have sent
to the cloud. It starts in milliseconds and needs no Docker. One collector also
covers Langfuse, LangSmith *and* plain OpenTelemetry, so the GenAI ecosystem
built on OTel (OpenLLMetry, OpenInference, the Vercel AI SDK, Pydantic AI)
works too.

## Research: what the SDKs actually send

The protocol details below were read from the SDK and server sources:

- langfuse-python 5.1.0, plus v2.60.10 for the legacy API
- langfuse-js 6.1.1
- the langfuse server
- langsmith-sdk (Python 0.14.9 and JS 0.10.9)
- the OpenTelemetry proto and exporters

The check below then confirmed every row against the real clients.

| SDK | Endpoint | Body |
| --- | --- | --- |
| Langfuse Python v3 and later | `POST /api/public/otel/v1/traces` | OTLP **protobuf**, **gzip** by default. The Python exporter has no JSON mode, so protobuf decoding is unavoidable |
| Langfuse JS v4 and later | same path | OTLP **JSON**, gzip on Node |
| Langfuse Python v2, JS v3, LiteLLM and others | `POST /api/public/ingestion` | `{batch: [{id, type, timestamp, body}]}`. Expects `207 {successes, errors}` |
| Langfuse scores | `POST /api/public/scores` (a JSON array from Python 5), or `score-create` inside the ingestion batch | JSON |
| LangSmith Python and JS | `GET /info`, then `POST /runs/multipart` | multipart parts `post.<id>`, `post.<id>.inputs`, `patch.<id>.outputs`, `feedback.<id>`, `attachment.<id>.<name>`. Python compresses the body with **zstd** when `/info` allows it; JS **streams** it (chunked) |
| LangSmith (fallback and older) | `POST /runs/batch`, `POST /runs`, `PATCH /runs/{id}`, `POST /feedback` | JSON |
| Any OpenTelemetry exporter | `POST /v1/traces` | OTLP protobuf or JSON |

Points that shaped the design:

- **The SDKs make no startup call that must succeed.** They only need the
  ingestion endpoints to return 2xx, so any API key can be accepted.
- **Langfuse picks the project from the key pair**, so the public key names the
  project here. LangSmith names it per run (`session_name`), and OTel per
  service (`service.name`).
- **LangSmith's `/info` replaces the Python SDK's defaults wholesale**, so every
  `batch_ingest_config` field the SDK indexes must be present.
- **The Python SDK warns on every run** when zstd is installed (it is, as a
  dependency) and the server declines it. The collector therefore decodes zstd
  (with `ruzstd`, pure Rust, so the Windows build needs no C toolchain) and
  advertises it.
- **LangSmith posts feedback at once but batches the runs it scores**, so a
  score often arrives before its run. Such scores are kept and attached when
  the run arrives.
- **Multipart boundaries are case-sensitive.** The JS SDK's boundary has upper
  case in it; an early build lowercased the header and rejected every JS
  upload. This is caught by a test now.
- Each dialect names things differently. The OTel mapping reads them in this
  order:
  1. Langfuse's `langfuse.observation.*` / `langfuse.trace.*`
  2. OpenInference's `openinference.span.kind`, `input.value`, `llm.token_count.*`
  3. the GenAI conventions' `gen_ai.operation.name`, `gen_ai.usage.*`, `gen_ai.prompt.N.*`, `gen_ai.input.messages`
  4. OpenLLMetry's `traceloop.*`
  5. the Vercel AI SDK's `ai.*`

  This is the same precedence the Langfuse server uses.

## What the beta does

**Collector** (`src-tauri/src/modules/llm_traces/server.rs`)
- An in-process HTTP/1.1 server (hyper) on 127.0.0.1 and ::1, on port 4318 by
  default. That is OTLP/HTTP's registered port, so an OTel app with default
  settings needs no configuration.
- **Docker.** On Linux, a container reaches the host through Docker's bridge
  (`host.docker.internal` maps to `docker0`, 172.17.0.1, via
  `extra_hosts: ["host.docker.internal:host-gateway"]`). Those packets arrive
  on the bridge, not on loopback, which is why a loopback-only listener
  refused them.
  - The collector therefore also binds the host's address on every Docker or
    Podman bridge (`docker0`, `br-<id>`, `podman0`).
  - It re-scans every 10 s, so a network that `docker compose up` creates
    later is picked up too.
  - These addresses are private to the machine; nothing on the LAN routes to
    them.
  - It's on by default, with a switch on the Connect tab.
  - Docker Desktop (macOS, Windows) relays `host.docker.internal` to the
    host's loopback itself, so there is nothing to bind there. That part is
    unverified on real hardware.
  - Verified on Docker Engine 29.9: from the default bridge and from compose
    networks, through `host.docker.internal` and through each network's
    gateway; from a network created after the collector started; and with the
    Langfuse and LangSmith JS SDKs running inside a `node:20` container.
- Off until you press Start. "Start with DevsPro" is a switch, and the
  collector stops when DevsPro quits.
- It accepts gzip, deflate and zstd bodies of up to 64 MB.
- It keeps a log of the last 30 requests (path, status, what was stored, how
  the body was encoded, and which address it came from, so a container shows
  as itself). This is the first place to look when an app "isn't showing
  up".

**Translation** (`langfuse.rs`, `langsmith.rs`, `otel.rs` → `model.rs`)
- Every protocol becomes one model: a trace of nested observations (span,
  generation, tool, chain, retriever, agent, event, …) with input, output,
  model, parameters, token usage, cost when the SDK reports it, level and error
  message, plus scores.
- OTLP protobuf is decoded with hand-declared `prost` messages, so the build
  needs no protoc.

**Store** (`store.rs`)
- A SQLite file of its own (`<app data>/llm-traces/traces.db`), not the shared
  `app.db`. Trace traffic never queues behind other modules, and the frozen
  migration list stays untouched.
- Upserts merge. A create and its later update become one row. A child that
  arrives before its parent still lands in the right trace. Metadata merges key
  by key. A trace's start and end widen to cover its observations. A root
  observation fills in the trace's name, input and output when the SDK set none
  at trace level.
- Token totals count LLM calls (`generation`, `embedding`) only. Wrappers such
  as agent spans often repeat their children's usage, and summing both would
  double it.
- It keeps the newest 2,000 traces by default (500 to 50,000 in settings). A
  single field over 512 KB is cut to a marked 32 KB excerpt.

**Page** (`src/modules/llm-traces/`)
- **Traces:** a live list (it refreshes as traces arrive) with search across
  name, input, output, user, session, tags and model. It filters by source,
  project and errors only, and shows latency, tokens, cost and model.
- **Trace view:** a waterfall of the observation tree beside the selected
  observation:
  - input and output, shown as a chat for any of the message shapes the SDKs
    use, with a JSON toggle
  - model and parameters
  - tokens, time to first token and cost
  - the error, metadata, and the raw OpenTelemetry attributes
- **Connect:** port, start and stop, retention, the Docker switch,
  environment snippets for each SDK filled in with the real address (or a
  docker-compose fragment, for an app in a container), and the request log.

## How it was verified

- **Unit tests:** 30 Rust tests (`cargo test --lib llm_traces`) and 10 frontend
  tests (`npx vitest run src/modules/llm-traces`). They cover each protocol's
  mapping, the merge rules, the multipart parser, compression, and an HTTP
  round trip through the running server.
- **The real SDKs:** `scripts/llm-traces-sdk-check/run.sh` installs the SDKs
  into a temp dir, starts the collector, sends one trace through each, and
  checks that every trace and score arrived. Last passing run (2026-11-03):

  | SDK | Version | Path |
  | --- | --- | --- |
  | Langfuse Python | 5.1.0, 4.18.0 | protobuf + gzip |
  | Langfuse Python | 2.60.10 | ingestion batch |
  | Langfuse JS | 6.1.0 | JSON + gzip |
  | LangSmith Python | 0.14.17 | multipart + zstd, feedback |
  | LangSmith JS | 0.11.7 | streamed multipart |
  | OpenTelemetry Python | 1.45.2 | GenAI conventions |

  Run it again when an SDK changes its wire format.
- **In the app:** the dev build was launched with the collector on 4318, the
  SDKs were run against it, and the list, the trace view and the Connect tab
  were checked on screen.

## Sample data

`scripts/llm-traces-seed/seed.py` (Python standard library only) fills a
running collector with a week of realistic traffic. It sends it over the real
protocols, so it exercises the collector end to end:

| App | Protocol | What it looks like |
| --- | --- | --- |
| Support bot | Langfuse | Multi-turn chats sharing a session. Each turn has intent classification, an order lookup, help-center retrieval and the reply, with the full history in the prompt. There's an A/B test between two reply models, occasional 429s with a retry, lookup timeouts, LLM-judge and thumbs-up scores. |
| Coding assistant | LangSmith | Pasted tracebacks, compiler errors and query plans, some log pastes over 100 KB, test runs as tool calls, and 529/timeout retries |
| Docs Q&A | OTel (GenAI conventions, OpenInference) | An HTTP span with embedding, vector search and chat; follow-up questions; a few upstream timeouts |
| SQL analytics agent | Langfuse | Tool-calling agent with `run_sql`, a bad query and its retry, and follow-ups |
| Travel planner | LangSmith | LangGraph-style agent → tools → agent, with parallel tool calls |
| Inbox summarizer | OTel (OpenLLMetry) | Long email threads with quoted replies |
| Translations | OTel (Gemini) | Many short single-call traces |
| Meeting notes | Langfuse | Transcripts of 120–900 KB, past the 512 KB per-field cap |

```sh
python3 scripts/llm-traces-seed/seed.py                # about 850 traces, to localhost:4318
python3 scripts/llm-traces-seed/seed.py --scale 5      # about 4,250
```

Raise "Keep" on the Connect tab first if you seed more than it holds.

**At volume** (debug build, 5,890 traces, 23,000 observations, a 69 MB
database):

| Operation | Time |
| --- | --- |
| A page of the list, with per-trace totals | 2–7 ms |
| The "errors only" filter | 27 ms |
| Free-text search, which scans inputs and outputs | 149 ms (the search box waits 250 ms after typing anyway) |
| Ingest | about 80 traces a second |

## Known limits of the beta

- Tracing only: no Langfuse prompt management, datasets, evaluations or media
  uploads. Calls to those return 404, which the SDKs log and carry on past.
- No OTLP over gRPC (port 4317).
- Cost is shown only when the SDK sends it; nothing is priced locally.
- Attachments (LangSmith) and media (Langfuse) aren't stored; the run notes
  which were left out.

## Plan: what comes next

In rough order of value:

1. **Reach from other machines.** An opt-in "listen on all interfaces"
   switch, with a generated key the collector then checks, for a phone or a
   second laptop on the LAN. Docker containers on this machine already work.
2. **Sessions and users.** Group traces by `session_id`, as a conversation
   view, and by user.
3. **Local cost.** A price table for common models, used when the SDK reports
   tokens but no cost, editable in settings.
4. **Compare and replay.** Diff two generations. Copy a generation as an HTTP
   request into the HTTP client module, to re-run it.
5. **Export.** Download a trace as JSON. Forward selected traces to a real
   Langfuse or LangSmith project (option C above, scoped to "send this one").
6. **Run real Langfuse in Docker** (option B) from the Docker or Databases
   module, for teams that want the full product locally.
7. **Prompt management stubs.** Serve `GET /api/public/v2/prompts/{name}` from
   prompts kept in DevsPro, so apps using Langfuse prompt management run
   offline.
8. OTLP over gRPC, if anyone asks.
