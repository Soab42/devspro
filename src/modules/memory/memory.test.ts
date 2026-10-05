import { describe, expect, it } from "vitest";
import type { AppDef, ResetTarget } from "./api";
import {
  cleanApp,
  formatMb,
  groupTargets,
  uniqueId,
  validateApp,
  watchdogMessage,
  watchdogStatus,
} from "./memory";

const app = (over: Partial<AppDef>): AppDef => ({
  id: "",
  name: "App",
  category: "Other",
  kind: "process",
  matcher: "app",
  start: null,
  flatpak_id: null,
  service: null,
  ...over,
});

const target = (pid: number, name: string, ram_mb: number, appName: string | null): ResetTarget => ({
  pid,
  start_time: pid * 10,
  name,
  command: name,
  ram_mb,
  app: appName,
});

describe("formatMb", () => {
  it("switches to GB from 1 GB up", () => {
    expect(formatMb(0)).toBe("0 MB");
    expect(formatMb(512)).toBe("512 MB");
    expect(formatMb(1023)).toBe("1023 MB");
    expect(formatMb(1024)).toBe("1.0 GB");
    expect(formatMb(2150)).toBe("2.1 GB");
  });
});

describe("watchdog copy", () => {
  it("states the threshold when on", () => {
    expect(watchdogStatus({ enabled: true, threshold_pct: 92, prefer_idle: true })).toBe(
      "Watchdog on — closes an app at 92%",
    );
    expect(watchdogStatus({ enabled: false, threshold_pct: 92, prefer_idle: true })).toBe("Watchdog off");
  });

  it("says why a close failed", () => {
    expect(
      watchdogMessage({
        at_ms: 0,
        app: "Zoom",
        ram_mb: 700,
        ram_pct: 97,
        reason: "largest",
        ok: false,
        message: "permission denied",
      }),
    ).toBe("Watchdog couldn't close Zoom (700 MB): permission denied — RAM was 97%");
  });
});

describe("groupTargets", () => {
  it("groups by app, else by process name, largest first", () => {
    const groups = groupTargets([
      target(1, "node", 100, null),
      target(2, "chrome", 900, "Chrome"),
      target(3, "node", 150, null),
      target(4, "chrome", 400, "Chrome"),
    ]);
    expect(groups.map((g) => [g.label, g.targets.length, g.ram_mb])).toEqual([
      ["Chrome", 2, 1300],
      ["node", 2, 250],
    ]);
  });
});

describe("catalog entries", () => {
  it("makes a unique slug from the name", () => {
    expect(uniqueId("PHP-FPM 8.3", [])).toBe("php-fpm-8-3");
    expect(uniqueId("Chrome", ["chrome", "chrome-2"])).toBe("chrome-3");
    expect(uniqueId("!!!", [])).toBe("app");
  });

  it("asks for the field each kind needs", () => {
    expect(validateApp(app({}))).toEqual({});
    expect(validateApp(app({ name: " ", matcher: "" }))).toEqual({
      name: "Give it a name.",
      matcher: "Needed to find its processes.",
    });
    expect(validateApp(app({ kind: "flatpak" })).flatpak_id).toBeDefined();
    expect(validateApp(app({ kind: "service", service: "nginx; rm -rf" })).service).toBe(
      "Letters, digits and @ . _ : - only.",
    );
    expect(validateApp(app({ kind: "service", service: "php8.3-fpm" }))).toEqual({});
  });

  it("clears the fields the chosen kind doesn't use", () => {
    expect(
      cleanApp(app({ kind: "flatpak", start: "x", flatpak_id: " org.gimp.GIMP ", service: "y" })),
    ).toMatchObject({ start: null, flatpak_id: "org.gimp.GIMP", service: null });
    expect(cleanApp(app({ start: "   " })).start).toBeNull();
  });
});
