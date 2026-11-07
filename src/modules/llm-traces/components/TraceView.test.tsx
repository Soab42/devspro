import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import type { Observation, TraceDetail } from "../api";

function obs(partial: Partial<Observation> & { id: string }): Observation {
  return {
    trace_id: "t1",
    parent_id: null,
    kind: "span",
    name: partial.id,
    start_us: 1_000_000,
    end_us: 2_000_000,
    completion_start_us: null,
    model: null,
    model_parameters: null,
    input: null,
    output: null,
    metadata: null,
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    cost: null,
    level: null,
    status_message: null,
    attributes: null,
    ...partial,
  };
}

const DETAIL: TraceDetail = {
  summary: {
    id: "t1",
    source: "langsmith",
    project: "support-bot",
    name: "answer-question",
    user_id: "u1",
    session_id: "s1",
    tags: ["beta"],
    start_us: 1_000_000,
    end_us: 3_000_000,
    updated_us: 3_000_000,
    observation_count: 3,
    generation_count: 1,
    input_tokens: 12,
    output_tokens: 4,
    total_tokens: 16,
    cost: null,
    error_count: 1,
    models: ["gpt-4o-mini"],
    input_preview: "Where is my order?",
    output_preview: "On its way",
  },
  input: { question: "Where is my order?" },
  output: { answer: "On its way" },
  metadata: null,
  release: null,
  version: null,
  environment: "dev",
  observations: [
    obs({ id: "root", kind: "chain", name: "answer-question", start_us: 1_000_000, end_us: 3_000_000 }),
    obs({
      id: "llm",
      parent_id: "root",
      kind: "generation",
      name: "ChatOpenAI",
      model: "gpt-4o-mini",
      input: [
        { role: "system", content: "You are a support agent." },
        { role: "user", content: "Where is my order?" },
      ],
      output: { role: "assistant", content: "On its way" },
      input_tokens: 12,
      output_tokens: 4,
      total_tokens: 16,
    }),
    obs({ id: "tool", parent_id: "root", kind: "tool", name: "lookup_order", level: "ERROR", status_message: "timeout" }),
  ],
  scores: [{ id: "sc", observation_id: null, name: "helpful", value: 1, string_value: null, comment: null, time_us: 1 }],
};

vi.mock("../api", () => ({
  getTrace: vi.fn(async () => DETAIL),
  deleteTraces: vi.fn(async () => 1),
  onIngested: vi.fn(async () => () => {}),
}));

vi.mock("../../../app/ConfirmContext", () => ({
  useConfirm: () => vi.fn(async () => true),
}));

const { TraceView } = await import("./TraceView");

describe("TraceView", () => {
  it("shows the trace, its observation tree and the trace's own input and scores", async () => {
    render(<TraceView id="t1" onBack={() => {}} />);
    expect(await screen.findAllByText("answer-question")).not.toHaveLength(0);
    const tree = screen.getByRole("tree", { name: "Observations" });
    const items = Array.from(tree.querySelectorAll('[role="treeitem"]')).map((el) => el.getAttribute("title"));
    expect(items).toEqual(["answer-question", "answer-question", "ChatOpenAI", "lookup_order"]);
    expect(screen.getByText("helpful: 1")).toBeTruthy();
    expect(screen.getByText(/Where is my order\?/)).toBeTruthy();
  });

  it("opens a generation as a chat, and says why a failed step failed", async () => {
    render(<TraceView id="t1" onBack={() => {}} />);
    fireEvent.click(await screen.findByTitle("ChatOpenAI"));
    expect(screen.getByText("You are a support agent.")).toBeTruthy();
    expect(screen.getByText("system")).toBeTruthy();
    expect(screen.getByText("gpt-4o-mini")).toBeTruthy();

    fireEvent.click(screen.getByTitle("lookup_order"));
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(screen.getByText("timeout")).toBeTruthy();
  });
});
