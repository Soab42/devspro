import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { HistoryEntry, ResultSet } from "../api";

const listHistory = vi.fn<() => Promise<HistoryEntry[]>>();
const runQuery =
  vi.fn<(c: string, d: string, sql: string, w: boolean) => Promise<ResultSet>>();

vi.mock("../api", () => ({
  listHistory: () => listHistory(),
  clearHistory: vi.fn(async () => undefined),
  runQuery: (c: string, d: string, sql: string, w: boolean) =>
    runQuery(c, d, sql, w),
  exportResult: vi.fn(async () => "exported"),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(async () => null) }));

import QueryTab from "./QueryTab";

const EMPTY: ResultSet = { columns: [], rows: [], total: null };

const entry = (sql: string, ok = true): HistoryEntry => ({
  id: sql,
  connection: "shop",
  database: "shop",
  sql,
  ok,
  rows: 1,
  duration_ms: 4,
  ran_at: "2026-09-23T00:00:00Z",
});

function renderTab(writable = true) {
  render(
    <QueryTab
      connection="shop"
      database="shop"
      dialect="mysql"
      schema={{ users: ["id", "name"] }}
      writable={writable}
    />,
  );
}

describe("QueryTab", () => {
  beforeEach(() => {
    listHistory.mockReset().mockResolvedValue([]);
    runQuery.mockReset().mockResolvedValue(EMPTY);
  });

  /**
   * Regression: the editor's extension set failed to build at all, because two
   * copies of @codemirror/state were installed and every `instanceof` across
   * them returned false. The console threw the moment it was opened — so the
   * cheapest useful assertion here is that it mounts.
   */
  it("mounts its editor", async () => {
    renderTab();
    expect(await screen.findByLabelText("SQL")).toBeTruthy();
  });

  it("has nothing to run until something is written", async () => {
    renderTab();
    const run = await screen.findByRole("button", { name: "Run" });
    expect(run).toHaveProperty("disabled", true);
  });

  describe("sheets", () => {
    it("opens another, and can close it again", async () => {
      renderTab();
      await screen.findByRole("tab", { name: /Query/ });
      const before = screen.getAllByRole("tab").length;

      fireEvent.click(screen.getByLabelText("New query sheet"));
      await waitFor(() =>
        expect(screen.getAllByRole("tab").length).toBe(before + 1),
      );

      // The close button only exists once there is more than one sheet.
      fireEvent.click(screen.getAllByLabelText(/^Close Query/)[0]);
      await waitFor(() =>
        expect(screen.getAllByRole("tab").length).toBe(before),
      );
    });

    it("keeps the last sheet uncloseable", async () => {
      renderTab();
      await screen.findByRole("tab", { name: /Query/ });
      expect(screen.queryByLabelText(/^Close Query/)).toBeNull();
    });
  });

  describe("history", () => {
    it("lists what ran, marking what failed", async () => {
      listHistory.mockResolvedValue([entry("SELECT 1"), entry("SELECT boom", false)]);
      renderTab();
      fireEvent.click(await screen.findByRole("button", { name: "History" }));

      expect(await screen.findByText("SELECT 1")).toBeTruthy();
      expect(screen.getByText("failed")).toBeTruthy();
    });

    /** Into a new sheet, so it can't overwrite what you were writing. */
    it("reopens a statement in a new sheet rather than the current one", async () => {
      listHistory.mockResolvedValue([entry("SELECT 1")]);
      renderTab();
      const before = (await screen.findAllByRole("tab")).length;

      fireEvent.click(screen.getByRole("button", { name: "History" }));
      fireEvent.click(await screen.findByText("SELECT 1"));

      await waitFor(() =>
        expect(screen.getAllByRole("tab").length).toBe(before + 1),
      );
      // Run is live now, and runs the statement that was reopened.
      fireEvent.click(screen.getByRole("button", { name: "Run" }));
      await waitFor(() => expect(runQuery).toHaveBeenCalled());
      expect(runQuery.mock.calls[0][2]).toBe("SELECT 1");
    });
  });

  describe("write mode", () => {
    it("says where writes are turned on when the connection is read-only", async () => {
      renderTab(false);
      expect(
        await screen.findByText(/switch it to Read & write/i),
      ).toBeTruthy();
    });
  });
});
