"""Sends one trace through each Python SDK to a running DevsPro collector.

Run by run.sh, which starts the collector and checks what it stored. Each
part runs only when its SDK is importable, so one virtualenv per SDK
generation can share this file:

    langfuse >= 3   OTLP protobuf (gzip) to /api/public/otel/v1/traces
    langfuse <  3   JSON batches to /api/public/ingestion
    langsmith       multipart to /runs/multipart, after GET /info
    opentelemetry   plain OTLP protobuf to /v1/traces, GenAI conventions

No model is called: every "LLM" response here is made up, so the check
needs no API key and costs nothing.
"""

import os
import sys

BASE = os.environ.get("COLLECTOR", "http://localhost:4399")
TAG = os.environ.get("CHECK_TAG", "sdk-check")
ran = []


def langfuse_v3_plus():
    os.environ.update(
        LANGFUSE_PUBLIC_KEY="pk-lf-devspro-check",
        LANGFUSE_SECRET_KEY="sk-lf-devspro-check",
        LANGFUSE_BASE_URL=BASE,
        LANGFUSE_HOST=BASE,
    )
    from langfuse import get_client, observe, propagate_attributes

    langfuse = get_client()

    @observe(as_type="generation", name="fake-llm")
    def fake_llm(question):
        langfuse.update_current_generation(
            model="gpt-4o-mini",
            model_parameters={"temperature": 0.1},
            usage_details={"input": 21, "output": 7},
            cost_details={"total": 0.000123},
        )
        return {"role": "assistant", "content": f"Answer to: {question}"}

    @observe(name="retrieve", as_type="retriever")
    def retrieve(question):
        return ["doc-1", "doc-2"]

    @observe(name="langfuse-v3-pipeline")
    def pipeline(question):
        with propagate_attributes(user_id="user-42", session_id="session-7", tags=[TAG, "langfuse"], trace_name="langfuse-otel-check"):
            docs = retrieve(question)
            answer = fake_llm([{"role": "system", "content": "Be brief."}, {"role": "user", "content": question}])
            try:
                with langfuse.start_as_current_observation(as_type="tool", name="broken-tool"):
                    raise ValueError("tool exploded")
            except ValueError:
                pass
            return {"answer": answer["content"], "docs": docs}

    pipeline("What does DevsPro do?")
    with langfuse.start_as_current_observation(as_type="span", name="scored-span") as span:
        span.update(input="score me", output="ok")
        langfuse.score_current_trace(name="helpful", value=1.0, comment="from the check")
    langfuse.flush()
    ran.append("langfuse-otel")


def langfuse_v2():
    from langfuse import Langfuse

    lf = Langfuse(public_key="pk-lf-legacy-check", secret_key="sk-lf-legacy-check", host=BASE)
    trace = lf.trace(name="langfuse-v2-check", user_id="legacy-user", session_id="legacy-session", tags=[TAG], input={"q": "hi"})
    span = trace.span(name="prepare", input={"step": 1})
    generation = span.generation(
        name="legacy-llm",
        model="claude-3-haiku",
        input=[{"role": "user", "content": "hi"}],
        model_parameters={"max_tokens": 64},
    )
    generation.end(output="hello there", usage={"input": 5, "output": 3})
    span.end(output={"done": True})
    trace.update(output={"a": "hello there"})
    trace.score(name="legacy-score", value=0.5)
    lf.flush()
    ran.append("langfuse-ingestion")


def langsmith():
    os.environ.update(
        LANGSMITH_TRACING="true",
        LANGSMITH_ENDPOINT=BASE,
        LANGSMITH_API_KEY="lsv2-devspro-check",
        LANGSMITH_PROJECT="devspro-check",
    )
    from langsmith import Client, traceable
    from langsmith.run_helpers import get_current_run_tree

    client = Client()

    @traceable(run_type="llm", name="fake-chat-model", client=client, metadata={"ls_model_name": "gpt-4.1-nano", "ls_provider": "openai"})
    def chat(messages):
        return {
            "choices": [{"message": {"role": "assistant", "content": "LangSmith says hi"}}],
            "usage_metadata": {"input_tokens": 12, "output_tokens": 4, "total_tokens": 16},
        }

    @traceable(run_type="tool", name="calculator", client=client)
    def calculator(expression):
        return {"result": 4}

    @traceable(run_type="tool", name="failing-tool", client=client)
    def failing():
        raise RuntimeError("langsmith tool failed")

    @traceable(name="langsmith-check-chain", client=client, tags=[TAG, "langsmith"], metadata={"user_id": "ls-user", "thread_id": "ls-thread"})
    def chain(question):
        calculator("2+2")
        try:
            failing()
        except RuntimeError:
            pass
        out = chat([{"role": "user", "content": question}])
        run = get_current_run_tree()
        return {"answer": out["choices"][0]["message"]["content"], "run_id": str(run.id)}

    result = chain("Does LangSmith tracing reach DevsPro?")
    client.create_feedback(run_id=result["run_id"], key="correctness", score=1, comment="from the check")
    client.flush()
    ran.append("langsmith")


def opentelemetry():
    from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
    from opentelemetry.sdk.resources import Resource
    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import SimpleSpanProcessor

    provider = TracerProvider(resource=Resource.create({"service.name": "otel-check-service"}))
    provider.add_span_processor(SimpleSpanProcessor(OTLPSpanExporter(endpoint=f"{BASE}/v1/traces")))
    tracer = provider.get_tracer("devspro-check")
    with tracer.start_as_current_span("otel-agent-run") as root:
        root.set_attribute("gen_ai.operation.name", "invoke_agent")
        root.set_attribute("session.id", "otel-session")
        with tracer.start_as_current_span("chat gpt-4o") as llm:
            llm.set_attribute("gen_ai.operation.name", "chat")
            llm.set_attribute("gen_ai.system", "openai")
            llm.set_attribute("gen_ai.request.model", "gpt-4o")
            llm.set_attribute("gen_ai.request.temperature", 0.3)
            llm.set_attribute("gen_ai.usage.input_tokens", 30)
            llm.set_attribute("gen_ai.usage.output_tokens", 9)
            llm.set_attribute("gen_ai.prompt.0.role", "user")
            llm.set_attribute("gen_ai.prompt.0.content", "Summarise OTLP")
            llm.set_attribute("gen_ai.completion.0.role", "assistant")
            llm.set_attribute("gen_ai.completion.0.content", "Spans over HTTP.")
    provider.shutdown()
    ran.append("otel")


def importable(name):
    try:
        __import__(name)
        return True
    except ImportError:
        return False


if __name__ == "__main__":
    if importable("langfuse"):
        from importlib.metadata import version

        (langfuse_v3_plus if int(version("langfuse").split(".")[0]) >= 3 else langfuse_v2)()
    if importable("langsmith"):
        langsmith()
    if importable("opentelemetry.exporter.otlp.proto.http"):
        opentelemetry()
    print("sent:", ", ".join(ran) or "nothing")
    sys.exit(0 if ran else 1)
