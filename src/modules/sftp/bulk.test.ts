import { describe, expect, it } from "vitest";
import { countLabel, failureSummary, settleEach, zipName } from "./bulk";

const entry = (name: string) => ({ name });

describe("settleEach", () => {
  it("runs every entry in order and collects each failure without stopping", async () => {
    const seen: string[] = [];
    const failures = await settleEach([entry("a"), entry("b"), entry("c")], async (e) => {
      seen.push(e.name);
      if (e.name === "a") throw "permission denied";
      if (e.name === "c") throw new Error("not empty");
    });
    expect(seen).toEqual(["a", "b", "c"]);
    expect(failures).toEqual([
      { name: "a", error: "permission denied" },
      { name: "c", error: "not empty" },
    ]);
  });

  it("reports nothing when everything worked", async () => {
    expect(await settleEach([entry("a")], async () => undefined)).toEqual([]);
  });
});

describe("failureSummary", () => {
  it("says how many of how many, and why each one failed", () => {
    expect(
      failureSummary("delete", [{ name: "logs", error: "is it empty?" }], 1),
    ).toBe("Couldn't delete 1 of 1 item: logs (is it empty?)");
    expect(
      failureSummary(
        "delete",
        [
          { name: "a", error: "x" },
          { name: "b", error: "y" },
        ],
        5,
      ),
    ).toBe("Couldn't delete 2 of 5 items: a (x); b (y)");
  });
});

describe("countLabel", () => {
  it("agrees with the number", () => {
    expect(countLabel(1)).toBe("1 item");
    expect(countLabel(0)).toBe("0 items");
    expect(countLabel(12)).toBe("12 items");
  });
});

describe("zipName", () => {
  it("names the archive after a lone entry, else after the folder", () => {
    expect(zipName([entry("site")], "/srv", "host")).toBe("site.zip");
    expect(zipName([entry("a"), entry("b")], "/var/log/", "host")).toBe("log.zip");
    expect(zipName([entry("a"), entry("b")], "/", "web-1")).toBe("web-1.zip");
    expect(zipName([entry("a"), entry("b")], null, "web-1")).toBe("web-1.zip");
  });
});
