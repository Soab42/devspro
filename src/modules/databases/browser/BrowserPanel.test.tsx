import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { SavedConnection, SchemaInfo, TableInfo } from "../api";

const listTables =
  vi.fn<(c: string, d: string, s?: string) => Promise<TableInfo[]>>();
const listSchemas = vi.fn<() => Promise<SchemaInfo[]>>();
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
  listSchemas: () => listSchemas(),
  listTables: (c: string, d: string, s?: string) => listTables(c, d, s),
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
    listSchemas.mockReset().mockResolvedValue([]);
    localStorage.clear();
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
    expect(listTables).toHaveBeenCalledWith("shop", "billing", undefined);
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

  describe("PostgreSQL schemas", () => {
    const schemas: SchemaInfo[] = [
      { name: "audit", tables: 1, size_bytes: 2048 },
      { name: "public", tables: 2, size_bytes: 4096 },
    ];
    const bySchema = async (_c: string, _d: string, s?: string) => {
      if (s === "audit") return [table("audit.events")];
      if (s === "public") return [table("orders")];
      return [table("orders"), table("audit.events")];
    };

    beforeEach(() => {
      listSchemas.mockResolvedValue(schemas);
      listTables.mockImplementation(bySchema);
    });

    it("hides the schema picker when the server has no schemas, as MySQL", async () => {
      listSchemas.mockResolvedValue([]);
      renderPanel();
      await screen.findByText("orders");
      expect(screen.queryByLabelText("Schema")).toBeNull();
    });

    it("opens on public, with the prefix-free names", async () => {
      renderPanel();
      expect(await screen.findByText("orders")).toBeTruthy();
      expect(listTables).toHaveBeenLastCalledWith("shop", "shop", "public");
      expect(screen.getByLabelText("Schema")).toBeTruthy();
    });

    it("reloads when the schema changes and drops the old tables", async () => {
      renderPanel();
      await screen.findByText("orders");

      fireEvent.click(screen.getByLabelText("Schema"));
      fireEvent.click(await screen.findByRole("option", { name: /^audit/ }));

      await waitFor(() => expect(screen.getByText("events")).toBeTruthy());
      expect(screen.queryByText("orders")).toBeNull();
      expect(listTables).toHaveBeenLastCalledWith("shop", "shop", "audit");
    });

    it("deselects a table that is not in the new schema", async () => {
      renderPanel();
      fireEvent.click(await screen.findByText("orders"));
      await screen.findByText("grid:orders");

      fireEvent.click(screen.getByLabelText("Schema"));
      fireEvent.click(await screen.findByRole("option", { name: /^audit/ }));

      await waitFor(() => expect(screen.queryByText("grid:orders")).toBeNull());
    });

    it("shows qualified names under All schemas", async () => {
      renderPanel();
      await screen.findByText("orders");

      fireEvent.click(screen.getByLabelText("Schema"));
      fireEvent.click(await screen.findByRole("option", { name: "All schemas" }));

      expect(await screen.findByText("audit.events")).toBeTruthy();
    });

    it("uses the schema remembered for the connection and database", async () => {
      localStorage.setItem("devspro.databases.schema.shop/shop", "audit");
      renderPanel();
      expect(await screen.findByText("events")).toBeTruthy();
      expect(listTables).toHaveBeenLastCalledWith("shop", "shop", "audit");
      expect(screen.queryByText("orders")).toBeNull();
    });
  });
});
