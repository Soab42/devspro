import { describe, expect, it } from "vitest";
import {
  CPU_TEMP_LIMITS,
  formatBytes,
  formatCelsius,
  formatKb,
  formatRate,
  formatUptime,
  GPU_TEMP_LIMITS,
  percent,
  tempScale,
  tempSeverity,
} from "./utils";

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

describe("formatCelsius", () => {
  it("rounds to a whole degree", () => {
    expect(formatCelsius(72.4)).toBe("72°C");
    expect(formatCelsius(66)).toBe("66°C");
  });
});

const sensor = (celsius: number, high: number | null = null, critical: number | null = null) => ({
  label: "Package id 0",
  celsius,
  high_c: high,
  critical_c: critical,
});

describe("tempScale", () => {
  it("scales the track from 0 °C to the critical point", () => {
    // Default CPU critical is 95 °C.
    expect(tempScale(sensor(95), CPU_TEMP_LIMITS).percent).toBe(100);
    expect(tempScale(sensor(47.5), CPU_TEMP_LIMITS).percent).toBe(50);
  });

  it("clamps a reading past critical rather than overflowing the track", () => {
    expect(tempScale(sensor(120), CPU_TEMP_LIMITS).percent).toBe(100);
  });

  it("keeps a warning band when the driver reports max === crit", () => {
    // Intel's coretemp reports both as the throttle point (100 °C here), which
    // taken literally would leave nothing between "fine" and "shutting down".
    const scale = tempScale(sensor(72, 100, 100), CPU_TEMP_LIMITS);
    expect(scale.critAt).toBe(95);
    expect(scale.warnAt).toBe(80);
  });

  it("honours vendor thresholds that are stricter than the defaults", () => {
    const scale = tempScale(sensor(70, 70, 85), CPU_TEMP_LIMITS);
    expect(scale.critAt).toBe(85);
    expect(scale.warnAt).toBe(70);
  });
});

describe("tempSeverity", () => {
  it("judges a CPU against CPU limits", () => {
    expect(tempSeverity(sensor(60), CPU_TEMP_LIMITS)).toBe("good");
    expect(tempSeverity(sensor(82), CPU_TEMP_LIMITS)).toBe("warning");
    expect(tempSeverity(sensor(96), CPU_TEMP_LIMITS)).toBe("critical");
  });

  it("gives a GPU the extra headroom it is built for", () => {
    // 82 °C is a warning on a CPU and a perfectly normal load temp on a GPU.
    expect(tempSeverity(sensor(82), GPU_TEMP_LIMITS)).toBe("good");
    expect(tempSeverity(sensor(88), GPU_TEMP_LIMITS)).toBe("warning");
  });
});
