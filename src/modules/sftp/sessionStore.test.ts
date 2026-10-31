import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
import { useBackgroundSessions } from "../../app/backgroundSessions";
import type { SshHost } from "../ssh/api";
import {
  addSftpSession,
  rememberPlace,
  rememberedPlace,
  removeSftpSession,
  resetSftpSessions,
  useSftpSessions,
} from "./sessionStore";

const host = { id: "h", name: "web" } as SshHost;

describe("the SFTP session store", () => {
  it("keeps connections, counts them for the sidebar, and forgets one that is closed", () => {
    resetSftpSessions();
    const sessions = renderHook(() => useSftpSessions());
    const badge = renderHook(() => useBackgroundSessions("sftp"));

    addSftpSession({ id: "a", host });
    addSftpSession({ id: "b", host });
    sessions.rerender();
    badge.rerender();
    expect(sessions.result.current.sessions.map((s) => s.id)).toEqual(["a", "b"]);
    expect(sessions.result.current.active).toBe("b");
    expect(badge.result.current).toBe(2);

    rememberPlace("b", "remote", "/srv/www");
    expect(rememberedPlace("b", "remote")).toBe("/srv/www");

    removeSftpSession("b");
    sessions.rerender();
    badge.rerender();
    expect(sessions.result.current.active).toBe("a");
    expect(badge.result.current).toBe(1);
    expect(rememberedPlace("b", "remote")).toBeNull();
  });
});
