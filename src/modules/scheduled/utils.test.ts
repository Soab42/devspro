import { describe, expect, it } from "vitest";
import {
  CRON_PRESETS,
  cronError,
  describeCron,
  formatAbsolute,
  formatRelative,
  lastOutputLine,
  normaliseSchedule,
  parseCron,
  presetForSchedule,
} from "./utils";

describe("presets", () => {
  it("maps each preset's expression back to itself", () => {
    for (const p of CRON_PRESETS.filter((p) => p.expr)) {
      expect(presetForSchedule(p.expr)).toBe(p.value);
    }
  });

  it("tolerates extra whitespace and falls back to custom", () => {
    expect(presetForSchedule("  */5   * * * * ")).toBe("5min");
    expect(presetForSchedule("*/7 * * * *")).toBe("custom");
    expect(presetForSchedule("")).toBe("custom");
  });

  it("normalises schedules to single spaces", () => {
    expect(normaliseSchedule(" 0   3 * *   * ")).toBe("0 3 * * *");
  });

  it("every preset expression is valid", () => {
    for (const p of CRON_PRESETS.filter((p) => p.expr)) {
      expect(cronError(p.expr)).toBeNull();
    }
  });
});

describe("parseCron", () => {
  it("expands steps, ranges, lists and names", () => {
    const spec = parseCron("*/15 1-3 1,15 * mon-fri");
    expect(spec.minute.values).toEqual([0, 15, 30, 45]);
    expect(spec.minute.any).toBe(false);
    expect(spec.hour.values).toEqual([1, 2, 3]);
    expect(spec.dom.values).toEqual([1, 15]);
    expect(spec.month.any).toBe(true);
    expect(spec.dow.values).toEqual([1, 2, 3, 4, 5]);
  });

  it("accepts month and day names in any case", () => {
    const spec = parseCron("0 0 1 JAN,jul Sun");
    expect(spec.month.values).toEqual([1, 7]);
    expect(spec.dow.values).toEqual([0]);
  });

  it("treats 7 as Sunday", () => {
    expect(parseCron("0 0 * * 7").dow.values).toEqual([0]);
    expect(parseCron("0 0 * * 0,7").dow.values).toEqual([0]);
  });

  it("rejects malformed expressions with a message", () => {
    expect(cronError("* * * *")).toMatch(/5 fields/);
    expect(cronError("60 * * * *")).toMatch(/minute/);
    expect(cronError("* 24 * * *")).toMatch(/hour/);
    expect(cronError("* * 0 * *")).toMatch(/day of month/);
    expect(cronError("* * * 13 *")).toMatch(/month/);
    expect(cronError("* * * * 8")).toMatch(/weekday/);
    expect(cronError("*/0 * * * *")).toMatch(/step/);
    expect(cronError("5-1 * * * *")).toMatch(/backwards/);
    expect(cronError("abc * * * *")).toMatch(/not a number/);
    expect(cronError("")).toMatch(/got 0/);
  });
});

describe("describeCron", () => {
  const d = (expr: string) => describeCron(expr).text;

  it("describes the preset shapes", () => {
    expect(d("* * * * *")).toBe("Every minute");
    expect(d("*/5 * * * *")).toBe("Every 5 minutes");
    expect(d("0 * * * *")).toBe("Every hour");
    expect(d("30 * * * *")).toBe("Every hour at :30");
    expect(d("0 3 * * *")).toBe("At 03:00 every day");
    expect(d("0 3 * * 0")).toBe("At 03:00 on Sunday");
    expect(d("0 */6 * * *")).toBe("Every 6 hours");
  });

  it("describes day combinations", () => {
    expect(d("0 9 * * 1-5")).toBe("At 09:00 on weekdays");
    expect(d("0 9 * * 0,6")).toBe("At 09:00 on weekends");
    expect(d("0 9 * * mon,wed,fri")).toBe("At 09:00 on Monday, Wednesday and Friday");
    expect(d("0 0 1 * *")).toBe("At 00:00 on day 1 of the month");
    expect(d("0 0 1 jan *")).toBe("At 00:00 on day 1 of the month in January");
    expect(d("0 8,20 * * *")).toBe("At 08:00 and 20:00 every day");
    expect(d("*/10 * * * sat")).toBe("Every 10 minutes on Saturday");
    expect(d("0 0 15 * 0")).toBe("At 00:00 on day 15 of the month or on Sunday");
  });

  it("falls back to a generic sentence", () => {
    expect(d("1,2 3,4 * * *")).toBe("At minute 1 and 2 past hour 3 and 4 every day");
  });

  it("returns the error text for an invalid expression", () => {
    expect(describeCron("nope")).toEqual({ text: expect.stringMatching(/5 fields/), valid: false });
  });
});

describe("time formatting", () => {
  const now = Date.parse("2026-03-02T10:00:00Z");
  const iso = (offsetMs: number) => new Date(now + offsetMs).toISOString();

  it("formats relative times in both directions", () => {
    expect(formatRelative(null, now)).toBe("—");
    expect(formatRelative("garbage", now)).toBe("—");
    expect(formatRelative(iso(10_000), now)).toBe("in under a minute");
    expect(formatRelative(iso(-10_000), now)).toBe("just now");
    expect(formatRelative(iso(3 * 60_000), now)).toBe("in 3 min");
    expect(formatRelative(iso(-5 * 60_000), now)).toBe("5 min ago");
    expect(formatRelative(iso(2 * 3_600_000), now)).toBe("in 2 h");
    expect(formatRelative(iso(-3 * 86_400_000), now)).toBe("3 d ago");
  });

  it("formats absolute local times", () => {
    expect(formatAbsolute(null)).toBe("");
    expect(formatAbsolute("garbage")).toBe("");
    expect(formatAbsolute("2026-03-02T10:05:00Z")).toMatch(/^2026-03-0\d \d\d:\d\d$/);
  });
});

describe("lastOutputLine", () => {
  it("returns the last non-blank line", () => {
    expect(lastOutputLine("a\nb\n\n   \n")).toBe("b");
    expect(lastOutputLine("")).toBe("");
    expect(lastOutputLine("only  ")).toBe("only");
  });
});
