import { describe, expect, it } from "vitest";
import {
  applyChanges,
  countStatuses,
  diffEnv,
  emptySide,
  formatValue,
  isDirty,
  parseEnv,
  QuoteStyle,
  shortName,
  toChange,
} from "./envFile";

const values = (text: string) =>
  Object.fromEntries([...parseEnv(text).entries].map(([k, e]) => [k, e.value]));

describe("parseEnv", () => {
  it("reads plain KEY=value pairs and ignores blanks and comments", () => {
    expect(values("# header\nA=1\n\n  # indented comment\nB=two\n")).toEqual({ A: "1", B: "two" });
  });

  it("accepts an export prefix, dotted and dashed keys, and spaces around =", () => {
    const parsed = parseEnv("export A=1\nB.C-D = x\n");
    expect(values("export A=1\nB.C-D = x\n")).toEqual({ A: "1", "B.C-D": "x" });
    expect(parsed.entries.get("A")?.exported).toBe(true);
    expect(parsed.entries.get("B.C-D")?.exported).toBe(false);
  });

  it("treats quotes as syntax, so quoting alone is never a difference", () => {
    expect(values(`A=x\nB="x"\nC='x'\n`)).toEqual({ A: "x", B: "x", C: "x" });
  });

  it("resolves escapes in double quotes but keeps single quotes literal", () => {
    expect(values(`A="a\\nb\\t\\"q\\" \\\\"\nB='a\\nb'\n`)).toEqual({
      A: 'a\nb\t"q" \\',
      B: "a\\nb",
    });
  });

  it("keeps an unrecognised escape as written", () => {
    expect(values(`A="C:\\dir\\file"\n`)).toEqual({ A: "C:\\dir\\file" });
  });

  it("reads a quoted value that spans lines", () => {
    const parsed = parseEnv(`KEY="-----BEGIN\nabc\n-----END"\nNEXT=1\n`);
    expect(parsed.entries.get("KEY")?.value).toBe("-----BEGIN\nabc\n-----END");
    expect(parsed.entries.get("KEY")).toMatchObject({ line: 1, endLine: 3 });
    expect(parsed.entries.get("NEXT")?.value).toBe("1");
  });

  it("starts an inline comment only after whitespace", () => {
    expect(
      values("A=plain # note\nB=#fff\nC=http://x/#top\nD=\"a # b\" # note\nE= # only a comment\n"),
    ).toEqual({ A: "plain", B: "#fff", C: "http://x/#top", D: "a # b", E: "" });
  });

  it("keeps an empty value distinct from a missing key", () => {
    const parsed = parseEnv("A=\nB=''\n");
    expect(parsed.entries.get("A")?.value).toBe("");
    expect(parsed.entries.get("B")?.value).toBe("");
    expect(parsed.entries.has("C")).toBe(false);
  });

  it("handles CRLF line endings and a BOM", () => {
    expect(values("\uFEFFA=1\r\nB=2\r\n")).toEqual({ A: "1", B: "2" });
  });

  it("lets the last definition of a key win and reports the repeat", () => {
    const parsed = parseEnv("A=1\nB=2\nA=3\n");
    expect(parsed.entries.get("A")?.value).toBe("3");
    expect(parsed.issues).toEqual([{ line: 3, message: expect.stringContaining("line 1") }]);
  });

  it("reports lines it cannot use instead of dropping them silently", () => {
    const parsed = parseEnv("A=1\nJUSTAKEY\n1BAD=x\n=novalue\n");
    expect(values("A=1\nJUSTAKEY\n1BAD=x\n=novalue\n")).toEqual({ A: "1" });
    expect(parsed.issues.map((i) => i.line)).toEqual([2, 3, 4]);
  });

  it("reads an unclosed quote as plain text and says so", () => {
    const parsed = parseEnv(`A="oops\nB=2\n`);
    expect(parsed.entries.get("A")?.value).toBe(`"oops`);
    expect(parsed.entries.get("B")?.value).toBe("2");
    expect(parsed.issues).toHaveLength(1);
    expect(parsed.issues[0].message).toContain("never closed");
  });

  it("returns nothing for empty input", () => {
    expect(parseEnv("").entries.size).toBe(0);
    expect(parseEnv("\n\n# only a comment\n").entries.size).toBe(0);
  });
});

describe("diffEnv", () => {
  const diff = (a: string, b: string, keysOnly = false) =>
    diffEnv(parseEnv(a), parseEnv(b), { keysOnly }).map((r) => [r.key, r.status]);

  it("does not care where in the file a variable sits", () => {
    expect(diff("A=1\nB=2\nC=3\n", "C=3\nA=1\nB=2\n")).toEqual([
      ["A", "same"],
      ["B", "same"],
      ["C", "same"],
    ]);
  });

  it("finds variables present on only one side", () => {
    expect(diff("A=1\nNEW_LEFT=x\n", "A=1\nNEW_RIGHT=y\n")).toEqual([
      ["A", "same"],
      ["NEW_LEFT", "only-left"],
      ["NEW_RIGHT", "only-right"],
    ]);
  });

  it("finds a same-named variable with a different value", () => {
    expect(diff("A=1\n", "A=2\n")).toEqual([["A", "changed"]]);
  });

  it("ignores quoting, comments and blank lines when comparing", () => {
    expect(diff(`A=1 # note\n\n# c\nB="x y"\n`, `B='x y'\nA=1\n`)).toEqual([
      ["A", "same"],
      ["B", "same"],
    ]);
  });

  it("is case-sensitive about keys", () => {
    expect(diff("Api=1\n", "API=1\n")).toEqual([
      ["API", "only-right"],
      ["Api", "only-left"],
    ]);
  });

  it("calls an empty value different from a missing key, and from a set one", () => {
    expect(diff("A=\nB=\n", "A=1\n")).toEqual([
      ["A", "changed"],
      ["B", "only-left"],
    ]);
  });

  it("collapses value differences into 'same' when comparing keys only", () => {
    expect(diff("A=1\nB=2\n", "A=9\n", true)).toEqual([
      ["A", "same"],
      ["B", "only-left"],
    ]);
  });

  it("compares the effective value when a key repeats within a file", () => {
    expect(diff("A=old\nA=new\n", "A=new\n")).toEqual([["A", "same"]]);
  });

  it("counts every status", () => {
    const rows = diffEnv(parseEnv("A=1\nB=2\nC=3\n"), parseEnv("A=1\nB=9\nD=4\n"));
    expect(countStatuses(rows)).toEqual({ same: 1, changed: 1, "only-left": 1, "only-right": 1 });
  });
});

describe("formatValue", () => {
  const SAMPLES = [
    "",
    "plain",
    "with space",
    " padded ",
    "has # hash",
    "#fff",
    "quote\"inside",
    "it's",
    "both ' and \"",
    "$HOME/${X}",
    "back\\slash",
    "trailing\\",
    "C:\\dir\\new",
    "literal\\nescape",
    "\\\"",
    "line1\nline2",
    "cr\r\nlf",
    "back\\\nnewline",
    "\"leading",
    "'leading",
    "=equals=",
  ];

  it.each(["none", "single", "double"] as QuoteStyle[])(
    "round-trips every awkward value through the parser as %s",
    (style) => {
      for (const value of SAMPLES) {
        const line = `K=${formatValue(value, style)}\n`;
        expect(values(line), `${style}: ${JSON.stringify(value)} → ${line}`).toEqual({ K: value });
      }
    },
  );

  it("leaves a bare-safe value bare, and keeps single quotes when it can", () => {
    expect(formatValue("abc", "none")).toBe("abc");
    expect(formatValue("a$b", "single")).toBe("'a$b'");
    expect(formatValue("a b", "none")).toBe("a b");
  });

  it("falls back to double quotes where the requested style can't hold the value", () => {
    expect(formatValue("it's", "single")).toBe(`"it's"`);
    expect(formatValue("a\nb", "none")).toBe(`"a\\nb"`);
  });
});

describe("applyChanges", () => {
  const change = (text: string, ...pairs: [string, string, QuoteStyle?][]) =>
    applyChanges(
      text,
      pairs.map(([key, value, quote]) => ({ key, value, quote: quote ?? "none" })),
    );

  it("appends a missing variable at the bottom and leaves the rest untouched", () => {
    const before = "# db\nDB=1\n\n# app\nAPP=2\n";
    expect(change(before, ["NEW", "x"])).toBe(before + "NEW=x\n");
  });

  it("rewrites a changed variable in place, keeping its position", () => {
    expect(change("A=1\nB=2\nC=3\n", ["B", "9"])).toBe("A=1\nB=9\nC=3\n");
  });

  it("keeps export and an inline comment on a rewritten line", () => {
    expect(change("export A=1 # keep me\n", ["A", "2"])).toBe("export A=2 # keep me\n");
  });

  it("collapses a multi-line value and keeps later line numbers right", () => {
    const before = 'A="x\ny\nz"\nB=1\nC=2\n';
    expect(change(before, ["A", "one"], ["C", "9"])).toBe("A=one\nB=1\nC=9\n");
  });

  it("rewrites only the last of a repeated key — the one that counts", () => {
    expect(change("A=1\nA=2\n", ["A", "9"])).toBe("A=1\nA=9\n");
  });

  it("applies several changes at once", () => {
    expect(change("A=1\nB=2\n", ["B", "9"], ["X", "1"], ["Y", "2"])).toBe("A=1\nB=9\nX=1\nY=2\n");
  });

  it("starts a new file when the text is empty", () => {
    expect(change("", ["A", "1"])).toBe("A=1\n");
  });

  it("adds a newline before appending to a file that lacked one", () => {
    expect(change("A=1", ["B", "2"])).toBe("A=1\nB=2\n");
  });

  it("does not add a trailing newline it wasn't asked to when only rewriting", () => {
    expect(change("A=1", ["A", "2"])).toBe("A=2");
  });

  it("preserves CRLF line endings and a BOM", () => {
    expect(change("\uFEFFA=1\r\nB=2\r\n", ["B", "9"], ["C", "3"])).toBe(
      "\uFEFFA=1\r\nB=9\r\nC=3\r\n",
    );
  });

  it("returns the text unchanged when there is nothing to apply", () => {
    const text = "A=1\r\n# x";
    expect(applyChanges(text, [])).toBe(text);
  });

  it("makes the target agree with the source once every row is copied across", () => {
    const left = parseEnv(`A=1\nB="two words"\nC='$literal'\nONLY_LEFT=x\n`);
    const rightText = "B=old\nA=1\nONLY_RIGHT=y\n";
    const rows = diffEnv(left, parseEnv(rightText)).filter(
      (r) => r.status === "only-left" || r.status === "changed",
    );
    const merged = applyChanges(rightText, rows.map((r) => toChange(r.left!)));

    const after = diffEnv(left, parseEnv(merged));
    expect(after.filter((r) => r.status === "changed" || r.status === "only-left")).toEqual([]);
    // Right's own extra variable survives; only left's are pushed across.
    expect(after.find((r) => r.key === "ONLY_RIGHT")?.status).toBe("only-right");
    // The quote style of the source came with the value.
    expect(merged).toContain("C='$literal'");
  });
});

describe("EnvSide helpers", () => {
  it("is dirty only for a file-backed side whose text has moved on from disk", () => {
    expect(isDirty(emptySide())).toBe(false);
    expect(isDirty({ text: "A=1", path: null, disk: null })).toBe(false);
    expect(isDirty({ text: "A=1", path: "/x/.env", disk: "A=1" })).toBe(false);
    expect(isDirty({ text: "A=2", path: "/x/.env", disk: "A=1" })).toBe(true);
  });

  it("names a file by its basename, adding the folder only when two collide", () => {
    expect(shortName("/a/api/.env", "/a/web/.env.example")).toBe(".env");
    expect(shortName("/a/api/.env", "/a/web/.env")).toBe("api/.env");
    expect(shortName("C:\\a\\api\\.env", null)).toBe(".env");
    expect(shortName(null, "/x/.env")).toBeNull();
  });
});
