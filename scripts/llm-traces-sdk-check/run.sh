#!/usr/bin/env bash
# Checks the LLM Traces collector against the real Langfuse, LangSmith and
# OpenTelemetry SDKs, not just against the formats its unit tests encode.
#
#   scripts/llm-traces-sdk-check/run.sh            # everything
#   SKIP_JS=1 scripts/llm-traces-sdk-check/run.sh  # Python only
#
# It installs the SDKs into WORK (a temp dir by default; set WORK to reuse one
# between runs), starts the collector on PORT with a throwaway database,
# sends one trace through each SDK, then checks every expected trace arrived.
# Needs uv, and node/npm unless SKIP_JS is set. No API keys: nothing calls a model.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
work="${WORK:-$(mktemp -d)}"
port="${PORT:-4399}"
db="$work/traces.db"
mkdir -p "$work"
rm -f "$db" "$db"-* "$db.stop"
echo "work dir: $work"

if [ ! -x "$work/py/bin/python" ]; then
  uv venv -q "$work/py"
  uv pip install -q --python "$work/py/bin/python" langfuse langsmith opentelemetry-sdk opentelemetry-exporter-otlp-proto-http
fi
if [ ! -x "$work/py-legacy/bin/python" ]; then
  uv venv -q "$work/py-legacy"
  uv pip install -q --python "$work/py-legacy/bin/python" "langfuse<3"
fi
if [ -z "${SKIP_JS:-}" ] && [ ! -d "$work/js/node_modules/langsmith" ]; then
  mkdir -p "$work/js"
  (cd "$work/js" && npm init -y >/dev/null &&
    npm install --silent --no-audit --no-fund --legacy-peer-deps \
      langsmith @langfuse/tracing @langfuse/otel @opentelemetry/sdk-trace-node@2 '@opentelemetry/api@~1.10.0' \
      @opentelemetry/exporter-trace-otlp-http @opentelemetry/otlp-exporter-base)
fi

echo "starting the collector on port $port (the first build takes a while)"
(cd "$repo/src-tauri" &&
  LLM_TRACES_PORT="$port" LLM_TRACES_DB="$db" LLM_TRACES_SECS=900 \
    cargo test --lib llm_traces::server::tests::serve -- --ignored --nocapture) >"$work/collector.log" 2>&1 &
collector=$!
trap 'touch "$db.stop"; wait $collector 2>/dev/null || true' EXIT
until grep -q "collector listening" "$work/collector.log" 2>/dev/null; do
  if ! kill -0 "$collector" 2>/dev/null; then
    cat "$work/collector.log"
    exit 1
  fi
  sleep 2
done

export COLLECTOR="http://localhost:$port"
(cd "$here" && "$work/py/bin/python" -I check_python.py)
(cd "$here" && "$work/py-legacy/bin/python" -I check_python.py)
if [ -z "${SKIP_JS:-}" ]; then
  cp "$here/check_js.mjs" "$work/js/" && (cd "$work/js" && node check_js.mjs)
fi

touch "$db.stop"
wait "$collector" || true
trap - EXIT
echo
grep -E " -> " "$work/collector.log" || true
echo

expected="langfuse-otel-check scored-span langsmith-check-chain otel-agent-run langfuse-v2-check"
[ -z "${SKIP_JS:-}" ] && expected="$expected langfuse-js-check langsmith-js-check"
python3 -I - "$db" $expected <<'EOF'
import sqlite3, sys
db, expected = sys.argv[1], sys.argv[2:]
conn = sqlite3.connect(db)
names = {name for (name,) in conn.execute("select name from traces")}
scores = {name for (name,) in conn.execute("select name from scores where trace_id is not null")}
missing = [n for n in expected if n not in names]
missing += [f"score {s}" for s in ("helpful", "correctness", "legacy-score") if s not in scores]
for row in conn.execute("select source, name, (select count(*) from observations o where o.trace_id = t.id) from traces t order by start_us"):
    print("  %-10s %-24s %d observations" % row)
if missing:
    print("MISSING:", ", ".join(missing))
    sys.exit(1)
print("all expected traces and scores arrived")
EOF
