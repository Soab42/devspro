import { describe, expect, it, vi } from "vitest";
import { dockerBlockedStatus } from "./blocked";

const missing = { found: false, daemon_reachable: false, version: null, error: null };
const stopped = { found: true, daemon_reachable: false, version: null, error: "pipe not found" };
const running = { found: true, daemon_reachable: true, version: "27.0", error: null };

describe("dockerBlockedStatus", () => {
  it("offers Apps when Docker is missing, with Recheck beside it", () => {
    const onOpenApps = vi.fn();
    const status = dockerBlockedStatus(missing, { onRecheck: vi.fn(), onOpenApps });
    expect(status?.kind).toBe("blocked");
    if (status?.kind !== "blocked") return;
    expect(status.title).toBe("Docker isn't installed");
    expect(status.action?.label).toBe("Install from Apps");
    status.action?.onClick();
    expect(onOpenApps).toHaveBeenCalled();
    expect(status.secondaryAction?.label).toBe("Recheck");
  });

  it("offers Start when Docker is installed but not running", () => {
    const onStart = vi.fn();
    const status = dockerBlockedStatus(stopped, { onRecheck: vi.fn(), onStart });
    if (status?.kind !== "blocked") throw new Error("expected blocked");
    expect(status.title).toBe("Docker isn't running");
    expect(status.action?.label).toMatch(/^Start Docker/);
    status.action?.onClick();
    expect(onStart).toHaveBeenCalled();
    expect(status.secondaryAction?.label).toBe("Recheck");
  });

  it("says it's starting while a start is in flight", () => {
    const status = dockerBlockedStatus(stopped, { onRecheck: vi.fn(), onStart: vi.fn(), starting: true });
    if (status?.kind !== "blocked") throw new Error("expected blocked");
    expect(status.title).toBe("Starting Docker…");
    expect(status.action?.loading).toBe(true);
  });

  it("falls back to Recheck alone where the caller offers nothing else", () => {
    const status = dockerBlockedStatus(stopped, { onRecheck: vi.fn() });
    if (status?.kind !== "blocked") throw new Error("expected blocked");
    expect(status.action?.label).toBe("Recheck");
    expect(status.secondaryAction).toBeUndefined();
  });

  it("is null once Docker answers, and before detection has run", () => {
    expect(dockerBlockedStatus(running, { onRecheck: vi.fn() })).toBeNull();
    expect(dockerBlockedStatus(null, { onRecheck: vi.fn() })).toBeNull();
  });
});
