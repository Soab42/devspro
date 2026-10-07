import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { CleanOutcome, Finding } from "../api";

const invoke = vi.hoisted(() => vi.fn());
const listeners = vi.hoisted(() => [] as ((e: { payload: unknown }) => void)[]);

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.push(cb);
    return () => {};
  }),
}));

import CachesTab from "./CachesTab";
import { resetTabState } from "../tabState";

const MIB = 1024 * 1024;

const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "caches",
  group: "JavaScript",
  label: "X",
  reason: "rebuilds itself",
  method: "delete",
  paths: ["/home/u/.cache/x"],
  bytes: 2048,
  risk: "safe",
  suggested: false,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

const FINDINGS = [
  f({ id: "npm", label: "npm cache", bytes: 300 * MIB, suggested: true }),
  f({ id: "gradle", label: "Gradle caches", group: "JVM", bytes: 500 * MIB, risk: "review", reason: "slow to download again" }),
  f({ id: "firefox", label: "Firefox cache", group: "Browsers and editors", bytes: 200 * MIB, suggested: true, blocked_by: "Firefox is running" }),
  f({ id: "pip", label: "pip cache", group: "Package managers", bytes: 10 * MIB }),
];

interface Backend {
  caches?: Finding[] | Error;
  system?: Finding[] | Error;
  outcomes?: CleanOutcome[];
}

function backend({ caches = FINDINGS, system = new Error("not implemented yet"), outcomes = [] }: Backend = {}) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "cleaner_scan_caches") {
      if (caches instanceof Error) throw caches;
      return caches;
    }
    if (cmd === "cleaner_scan_system") {
      if (system instanceof Error) throw system;
      return system;
    }
    if (cmd === "cleaner_clean_caches" || cmd === "cleaner_clean_system") return outcomes;
    return undefined;
  });
}

const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);
const scanButton = () => screen.getByRole("button", { name: /^(Scan|Rescan)$/ }) as HTMLButtonElement;

async function scan() {
  await act(async () => fireEvent.click(scanButton()));
  await screen.findByText("npm cache");
}

const box = (name: string) => screen.getByRole("checkbox", { name: `Select ${name}` }) as HTMLInputElement;

beforeEach(() => {
  resetTabState();
  invoke.mockReset();
  listeners.length = 0;
});
afterEach(cleanup);

describe("CachesTab", () => {
  it("scans on demand and groups findings by kind", async () => {
    backend();
    render(<CachesTab />);
    expect(calls("cleaner_scan_caches")).toHaveLength(0);
    await scan();
    expect(calls("cleaner_scan_caches")[0][1]).toEqual({ scanId: expect.any(String) });
    for (const group of ["JavaScript", "JVM", "Browsers and editors", "Package managers"]) {
      expect(screen.getByRole("region", { name: group })).toBeTruthy();
    }
    expect(screen.getByText("slow to download again")).toBeTruthy();
    expect(screen.getByText("Blocked: Firefox is running")).toBeTruthy();
    expect(box("Firefox cache").disabled).toBe(true);
  });

  it("shows live progress while scanning and cancels both scans", async () => {
    let finish: (v: Finding[]) => void = () => {};
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "cleaner_scan_caches") return new Promise<Finding[]>((r) => (finish = r));
      if (cmd === "cleaner_scan_system") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    render(<CachesTab />);
    await act(async () => fireEvent.click(scanButton()));
    const scanId = (calls("cleaner_scan_caches")[0][1] as { scanId: string }).scanId;
    await act(async () =>
      listeners[0]({ payload: { scan_id: scanId, tab: "caches", entries: 1234, bytes: 3 * MIB, current: "/home/u/.npm" } }),
    );
    expect(screen.getByText(/Scanning 1,234 entries/).textContent).toContain("3.0 MB");
    expect(screen.getByText("/home/u/.npm")).toBeTruthy();
    // Another scan's progress is ignored.
    await act(async () =>
      listeners[0]({ payload: { scan_id: "other", tab: "caches", entries: 9, bytes: 0, current: "/elsewhere" } }),
    );
    expect(screen.queryByText("/elsewhere")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls("cleaner_cancel_scan")).toHaveLength(2);
    expect(calls("cleaner_cancel_scan")[0][1]).toEqual({ scanId });
    await act(async () => finish([FINDINGS[0]]));
    await screen.findByText("npm cache");
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("selects only the suggested findings that are not blocked", async () => {
    backend();
    render(<CachesTab />);
    await scan();
    fireEvent.click(screen.getByRole("button", { name: "Select suggested" }));
    expect(box("npm cache").checked).toBe(true);
    expect(box("Firefox cache").checked).toBe(false);
    expect(box("Gradle caches").checked).toBe(false);
    expect(screen.getByRole("button", { name: /Clean selected/ }).textContent).toContain("300 MB");
  });

  it("reviews the selection, cleans through the caches command and rescans", async () => {
    backend({ outcomes: [{ id: "npm", ok: true, message: "ran npm cache clean --force", bytes_freed: 250 * MIB }] });
    render(<CachesTab />);
    await scan();
    const cleanButton = screen.getByRole("button", { name: /Clean selected/ }) as HTMLButtonElement;
    expect(cleanButton.disabled).toBe(true);
    fireEvent.click(box("npm cache"));
    fireEvent.click(box("Gradle caches"));
    fireEvent.click(screen.getByRole("button", { name: /Clean selected/ }));

    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getAllByText("/home/u/.cache/x")).toHaveLength(2);
    expect(calls("cleaner_clean_caches")).toHaveLength(0);
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: /^Clean / })));

    expect(calls("cleaner_clean_caches")[0][1]).toEqual({ ids: ["npm", "gradle"] });
    expect(calls("cleaner_clean_system")).toHaveLength(0);
    await waitFor(() => expect(within(dialog).getByText(/Freed/).textContent).toContain("250 MB"));
    await waitFor(() => expect(calls("cleaner_scan_caches")).toHaveLength(2));
  });

  it("sends system findings to the system clean command", async () => {
    const sys = f({ id: "journal", tab: "system", group: "Logs", label: "Journal", bytes: 5 * MIB });
    backend({ system: [sys], outcomes: [{ id: "journal", ok: true, message: "vacuumed", bytes_freed: 5 * MIB }] });
    render(<CachesTab />);
    await scan();
    fireEvent.click(box("npm cache"));
    fireEvent.click(box("Journal"));
    fireEvent.click(screen.getByRole("button", { name: /Clean selected/ }));
    const dialog = await screen.findByRole("dialog");
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: /^Clean / })));
    expect(calls("cleaner_clean_caches")[0][1]).toEqual({ ids: ["npm"] });
    expect(calls("cleaner_clean_system")[0][1]).toEqual({ ids: ["journal"] });
  });

  it("shows a failing system scan as an inline callout and keeps the caches", async () => {
    backend({ system: new Error("not implemented yet") });
    render(<CachesTab />);
    await scan();
    expect(screen.getByText(/System scan failed: not implemented yet/)).toBeTruthy();
    expect(screen.getByText("npm cache")).toBeTruthy();
    expect(box("npm cache").disabled).toBe(false);
  });

  it("reports a failed caches scan without breaking the tab", async () => {
    backend({ caches: new Error("HOME is not set"), system: [] });
    render(<CachesTab />);
    await act(async () => fireEvent.click(scanButton()));
    await screen.findByText("HOME is not set");
    expect(scanButton().textContent).toBe("Rescan");
  });
});
