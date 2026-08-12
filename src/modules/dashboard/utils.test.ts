import { describe, expect, it } from "vitest";
import { formatBytes, formatKb, formatRate, formatUptime, percent } from "./utils";

describe("formatBytes", () => {
  it("handles zero and negative as 0 B", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-5)).toBe("0 B");
  });

  it("picks the right unit", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 * 1024 * 3)).toBe("3.0 MB");
  });

  it("respects the decimals param", () => {
    expect(formatBytes(1536, 2)).toBe("1.50 KB");
  });
});

describe("formatKb", () => {
  it("converts KB to the same output as the equivalent bytes", () => {
    expect(formatKb(1024)).toBe(formatBytes(1024 * 1024));
  });
});

describe("formatRate", () => {
  it("appends /s to a byte size", () => {
    expect(formatRate(1024)).toBe("1.0 KB/s");
  });
});

describe("formatUptime", () => {
  it("omits days/hours when zero", () => {
    expect(formatUptime(90)).toBe("1m");
  });

  it("includes hours once minutes roll past an hour, and days once past a day", () => {
    expect(formatUptime(3661)).toBe("1h 1m");
    expect(formatUptime(90061)).toBe("1d 1h 1m");
  });
});

describe("percent", () => {
  it("returns 0 when total is zero or negative", () => {
    expect(percent(5, 0)).toBe(0);
    expect(percent(5, -1)).toBe(0);
  });

  it("computes the ratio as a percentage", () => {
    expect(percent(25, 100)).toBe(25);
    expect(percent(1, 3)).toBeCloseTo(33.33, 1);
  });
});
