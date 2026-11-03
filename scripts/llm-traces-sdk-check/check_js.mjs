// Sends one trace through each JavaScript SDK to a running DevsPro collector.
// Run by run.sh, from a folder where these are installed:
//
//   npm i langsmith @langfuse/tracing @langfuse/otel @opentelemetry/sdk-trace-node@2
//
// What it exercises that the Python check does not:
//   @langfuse/otel   OTLP *JSON*, gzipped, to /api/public/otel/v1/traces
//   langsmith        a *streamed* (chunked) multipart body to /runs/multipart
//
// No model is called; the "LLM" answers are made up.

const BASE = process.env.COLLECTOR ?? "http://localhost:4399";
const TAG = process.env.CHECK_TAG ?? "sdk-check";
const sent = [];

async function langfuse() {
  const { NodeTracerProvider } = await import("@opentelemetry/sdk-trace-node");
  const { LangfuseSpanProcessor } = await import("@langfuse/otel");
  const { startActiveObservation, startObservation, propagateAttributes } = await import("@langfuse/tracing");

  const processor = new LangfuseSpanProcessor({
    publicKey: "pk-lf-js-check",
    secretKey: "sk-lf-js-check",
    baseUrl: BASE,
  });
  const provider = new NodeTracerProvider({ spanProcessors: [processor] });
  provider.register();

  await startActiveObservation("js-pipeline", async (span) => {
    await propagateAttributes(
      { userId: "js-user", sessionId: "js-session", tags: [TAG, "langfuse-js"], traceName: "langfuse-js-check" },
      async () => {
        span.update({ input: { question: "Is JSON OTLP decoded?" } });
        const generation = startObservation(
          "js-llm",
          { model: "claude-sonnet", input: [{ role: "user", content: "Is JSON OTLP decoded?" }] },
          { asType: "generation" },
        );
        generation.update({ output: { role: "assistant", content: "Yes." }, usageDetails: { input: 8, output: 2 } }).end();
        span.update({ output: "done" });
      },
    );
  });
  await processor.forceFlush();
  await provider.shutdown();
  sent.push("langfuse-js");
}

async function langsmith() {
  process.env.LANGSMITH_TRACING = "true";
  const { Client } = await import("langsmith");
  const { traceable } = await import("langsmith/traceable");
  const client = new Client({ apiUrl: BASE, apiKey: "lsv2-js-check" });

  const llm = traceable(
    async (question) => ({
      content: `JS says: ${question}`,
      usage_metadata: { input_tokens: 6, output_tokens: 3, total_tokens: 9 },
    }),
    { name: "js-chat-model", run_type: "llm", client, metadata: { ls_model_name: "gpt-4o", ls_provider: "openai" } },
  );
  const chain = traceable(
    async (question) => {
      const out = await llm(question);
      return { answer: out.content };
    },
    { name: "langsmith-js-check", client, project_name: "devspro-check-js", tags: [TAG, "langsmith-js"] },
  );
  await chain("streamed multipart?");
  await client.awaitPendingTraceBatches();
  sent.push("langsmith-js");
}

for (const [name, run] of [["langfuse", langfuse], ["langsmith", langsmith]]) {
  try {
    await run();
  } catch (e) {
    console.error(`${name} check failed:`, e);
  }
}
console.log("sent:", sent.join(", ") || "nothing");
process.exit(sent.length === 2 ? 0 : 1);
