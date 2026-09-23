import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { SavedConnection, TableInfo } from "../api";

const listTables = vi.fn<(c: string, d: string) => Promise<TableInfo[]>>();
const listDatabaseNames = vi.fn<() => Promise<string[]>>();

const table = (name: string): TableInfo => ({
  name,
  kind: "table",
  rows_estimate: 0,
  size_bytes: 0,
});

vi.mock("../api", () => ({
  listConnections: vi.fn(async (): Promise<SavedConnection[]> => []),
  listDatabaseNames: () => listDatabaseNames(),
  listTables: (c: string, d: string) => listTables(c, d),
  fetchTableStructure: vi.fn(async () => ({
    columns: [],
    indexes: [],
    foreign_keys: [],
    referenced_by: [],
  })),
  getMode: vi.fn(async () => "ro"),
  setMode: vi.fn(async (_c: string, m: string) => m),
  dumpDatabase: vi.fn(async () => "dumped"),
  restoreDump: vi.fn(async () => "restored"),
}));

vi.mock("./DataTab", () => ({
  default: ({ table: name }: { table: string }) => <div>grid:{name}</div>,
}));
vi.mock("./StructureTab", () => ({ default: () => <div>structure</div> }));
vi.mock("./QueryTab", () => ({ default: () => <div>query</div> }));
vi.mock("../components/ConnectionsModal", () => ({ default: () => null }));
vi.mock("../../../app/ConfirmContext", () => ({
  useConfirm: () => vi.fn(async () => true),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(async () => null),
  open: vi.fn(async () => null),
}));

import BrowserPanel from "./BrowserPanel";

describe("BrowserPanel", () => {
  beforeEach(() => {
    listDatabaseNames.mockReset().mockResolvedValue(["shop", "billing"]);
    listTables.mockReset().mockImplementation(async (_c, database) =>
      database === "shop"
        ? [table("orders"), table("customers")]
        : [table("invoices")],
    );
  });

  const renderPanel = () =>
    render(<BrowserPanel managed={[{ base: "shop", running: true }]} />);

  it("opens on the database named after the connection", async () => {
    renderPanel();
    expect(await screen.findByText("orders")).toBeTruthy();
  });

  /**
   * Regression: the table list was fetched with `useResource`, which fires on
   * mount and when its `enabled` flips — neither of which happens when the
   * database changes. Picking a second database left the first one's tables in
   * the sidebar, listing tables that aren't there to open.
   */
  it("reloads the tables when the database changes", async () => {
    renderPanel();
    await screen.findByText("orders");

    fireEvent.click(screen.getByLabelText("Database"));
    fireEvent.click(await screen.findByRole("option", { name: "billing" }));

    await waitFor(() => expect(screen.getByText("invoices")).toBeTruthy());
    expect(listTables).toHaveBeenCalledWith("shop", "billing");
  });

  /** The old ones must not linger beside the new database's name. */
  it("does not leave the previous database's tables on screen", async () => {
    renderPanel();
    await screen.findByText("orders");

    fireEvent.click(screen.getByLabelText("Database"));
    fireEvent.click(await screen.findByRole("option", { name: "billing" }));

    await waitFor(() => expect(screen.queryByText("orders")).toBeNull());
  });

  it("clears the open table so the grid is not left on a missing one", async () => {
    renderPanel();
    fireEvent.click(await screen.findByText("orders"));
    expect(await screen.findByText("grid:orders")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Database"));
    fireEvent.click(await screen.findByRole("option", { name: "billing" }));

    await waitFor(() => expect(screen.queryByText("grid:orders")).toBeNull());
    expect(screen.getByText("Pick a table")).toBeTruthy();
  });

  it("reports a failure to list tables rather than showing an empty sidebar", async () => {
    listTables.mockRejectedValue(new Error("permission denied for schema"));
    renderPanel();
    expect(
      await screen.findByText(/permission denied for schema/),
    ).toBeTruthy();
  });
});
