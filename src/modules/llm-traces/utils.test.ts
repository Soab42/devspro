import { describe, expect, it } from "vitest";
import type { Observation } from "./api";
import { asMessages, buildTree, contentText, formatCost, formatDuration, formatTokens } from "./utils";

function obs(id: string, parent: string | null, start: number, end: number | null): Observation {
  return {
    id,
    trace_id: "t",
    parent_id: parent,
    kind: "span",
    name: id,
    start_us: start,
    end_us: end,
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
  };
}

describe("formatting", () => {
  it("picks a unit that fits the size", () => {
    expect(formatDuration(820)).toBe("820 µs");
    expect(formatDuration(312_400)).toBe("312 ms");
    expect(formatDuration(4_213_000)).toBe("4.21 s");
    expect(formatDuration(125_000_000)).toBe("2m 05s");
    expect(formatDuration(null)).toBe("-");
    expect(formatTokens(980)).toBe("980");
    expect(formatTokens(12_345)).toBe("12k");
    expect(formatTokens(1_500)).toBe("1.5k");
    expect(formatCost(0.000123)).toBe("$0.000123");
    expect(formatCost(0.42)).toBe("$0.42");
    expect(formatCost(12.5)).toBe("$12.50");
  });
});

describe("buildTree", () => {
  it("orders parents before children and siblings by start", () => {
    const rows = buildTree(
      [obs("b", "root", 30, 40), obs("root", null, 0, 100), obs("a", "root", 10, 20), obs("a1", "a", 12, 14)],
      0,
      100,
    );
    expect(rows.map((r) => `${r.obs.id}:${r.depth}`)).toEqual(["root:0", "a:1", "a1:2", "b:1"]);
    const a = rows[1];
    expect(a.offset).toBeCloseTo(0.1);
    expect(a.width).toBeCloseTo(0.1);
  });

  it("shows an observation whose parent never arrived as a root, and survives a cycle", () => {
    const rows = buildTree([obs("orphan", "missing", 5, 6), obs("x", "y", 1, 2), obs("y", "x", 1, 2)], 0, 10);
    expect(rows.map((r) => r.obs.id).sort()).toEqual(["orphan", "x", "y"]);
    expect(rows.find((r) => r.obs.id === "orphan")?.depth).toBe(0);
  });
});

describe("asMessages", () => {
  it("reads OpenAI-style message lists and {messages}", () => {
    expect(asMessages([{ role: "user", content: "hi" }])).toEqual([{ role: "user", text: "hi", toolCalls: undefined }]);
    expect(asMessages({ messages: [{ role: "system", content: "be brief" }] })?.[0].role).toBe("system");
  });

  it("reads content parts, LangChain's serialised messages and LangSmith's nested lists", () => {
    const parts = asMessages([{ role: "user", content: [{ type: "text", text: "look" }, { type: "image_url" }] }]);
    expect(parts?.[0].text).toBe("look\n[image]");
    const lc = asMessages({
      messages: [[{ lc: 1, type: "constructor", id: ["langchain", "schema", "messages", "AIMessage"], kwargs: { content: "yo" } }]],
    });
    expect(lc).toEqual([{ role: "assistant", text: "yo", toolCalls: undefined }]);
  });

  it("reads responses: OpenAI choices, LangChain generations, GenAI parts", () => {
    expect(asMessages({ choices: [{ message: { role: "assistant", content: "answer" } }] })?.[0].text).toBe("answer");
    expect(asMessages({ generations: [[{ text: "gen" }]] })?.[0]).toEqual({ role: "assistant", text: "gen", toolCalls: undefined });
    expect(asMessages([{ role: "user", parts: [{ type: "text", content: "p" }] }])?.[0].text).toBe("p");
  });

  it("refuses anything that isn't a chat", () => {
    expect(asMessages({ question: "q" })).toBeNull();
    expect(asMessages([1, 2])).toBeNull();
    expect(asMessages("text")).toBeNull();
    expect(asMessages([])).toBeNull();
    expect(asMessages([{ role: "user", content: "a" }, { nope: 1 }])).toBeNull();
  });

  it("keeps tool calls and stringifies structured content", () => {
    const [m] = asMessages([{ role: "assistant", content: null, tool_calls: [{ id: "c1" }] }])!;
    expect(m.toolCalls).toEqual([{ id: "c1" }]);
    expect(contentText({ a: 1 })).toBe('{\n  "a": 1\n}');
  });
});
