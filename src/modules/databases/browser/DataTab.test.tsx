import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";

import type { PendingEdit, ResultSet, TableStructure } from "../api";

const fetchTableRows = vi.fn<() => Promise<ResultSet>>();
const applyEdits =
  vi.fn<(c: string, d: string, t: string, e: PendingEdit[]) => Promise<string>>();

vi.mock("../api", () => ({
  fetchTableRows: () => fetchTableRows(),
  applyEdits: (c: string, d: string, t: string, e: PendingEdit[]) =>
    applyEdits(c, d, t, e),
  exportResult: vi.fn(async () => "exported"),
  addColumn: vi.fn(async () => "added"),
  // A real value the grid reads, not a function — leaving it out of the mock
  // makes it undefined and every column looks like a relation column.
  RELATION_PREFIX: "__rel_",
}));

// Typed with its parameters so the message it was called with can be asserted.
const confirm = vi.fn(async (_message: string, _options?: unknown) => true);
const navigate = vi.fn();
vi.mock("../../../app/ConfirmContext", () => ({ useConfirm: () => confirm }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(async () => null),
  open: vi.fn(async () => null),
}));

import DataTab from "./DataTab";

const RESULT: ResultSet = {
  columns: [
    { name: "id", type: "int" },
    { name: "name", type: "text" },
  ],
  rows: [
    ["1", "alice"],
    ["2", "bob"],
  ],
  total: 2,
};

/** A page that also carries a foreign key and one relation count column. */
const LINKED: ResultSet = {
  columns: [
    { name: "id", type: "int" },
    { name: "owner_id", type: "int" },
    { name: "__rel_0", type: "" },
  ],
  rows: [
    ["1", "7", "3"],
    ["2", "8", "0"],
  ],
  total: 2,
};

const LINKED_STRUCTURE: TableStructure = {
  columns: ["id", "owner_id"].map((name) => ({
    name,
    type: "int",
    nullable: false,
    default: "",
    primary_key: name === "id",
    extra: "",
  })),
  indexes: [],
  foreign_keys: [
    {
      name: "fk_owner",
      columns: ["owner_id"],
      references_table: "users",
      references_columns: ["id"],
      on_delete: "",
      on_update: "",
    },
  ],
  referenced_by: [
    {
      name: "fk_task_project",
      table: "tasks",
      columns: ["project_id"],
      references_columns: ["id"],
    },
  ],
};

const structure = (primary: string[]): TableStructure => ({
  columns: ["id", "name"].map((name) => ({
    name,
    type: "text",
    nullable: true,
    default: "",
    primary_key: primary.includes(name),
    extra: "",
  })),
  indexes: [],
  referenced_by: [],
  foreign_keys: [],
});

/** Renders, and waits for the first page to land. */
function renderTab(
  props: Partial<React.ComponentProps<typeof DataTab>> = {},
  settled = "alice",
) {
  render(
    <DataTab
      connection="shop"
      database="shop"
      table="users"
      objectKind="table"
      structure={structure(["id"])}
      writable
      engine="mysql"
      onNavigate={navigate}
      {...props}
    />,
  );
  return screen.findByText(settled);
}

/** Double-click a cell, type, and commit with Enter. */
async function editCell(text: string, value: string) {
  fireEvent.doubleClick(await screen.findByText(text));
  const input = screen.getByLabelText("Cell value");
  fireEvent.change(input, { target: { value } });
  fireEvent.keyDown(input, { key: "Enter" });
}

describe("DataTab", () => {
  beforeEach(() => {
    fetchTableRows.mockReset().mockResolvedValue(RESULT);
    applyEdits.mockReset().mockResolvedValue("saved 1 change");
    confirm.mockClear();
    navigate.mockClear();
  });

  it("shows the rows it fetched", async () => {
    await renderTab();
    expect(screen.getByText("bob")).toBeTruthy();
  });

  describe("read-only mode", () => {
    /** The guard is enforced in the backend; this is the UI agreeing with it. */
    it("offers no way to add or edit a row, and says why", async () => {
      await renderTab({ writable: false });
      expect(screen.queryByRole("button", { name: "Add row" })).toBeNull();
      expect(screen.getByText(/read-only mode/i)).toBeTruthy();
      // The cell is plain text, not the button that opens an editor.
      expect(screen.getByText("alice").tagName).not.toBe("BUTTON");
    });
  });

  describe("a table with no primary key", () => {
    it("explains that there is no way to name one row", async () => {
      await renderTab({ structure: structure([]) });
      expect(screen.getByText(/no primary key/i)).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Add row" })).toBeNull();
    });
  });

  describe("a view", () => {
    it("is never editable, even on a writable connection", async () => {
      await renderTab({ objectKind: "view" });
      expect(screen.getByText(/Views can't be edited/i)).toBeTruthy();
    });
  });

  describe("staging and saving", () => {
    it("counts unsaved rows and only then offers Save", async () => {
      await renderTab();
      expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
      await editCell("alice", "alicia");
      expect(await screen.findByText("1 unsaved row")).toBeTruthy();
      expect(screen.getByRole("button", { name: "Save" })).toBeTruthy();
    });

    it("sends only the changed cell, with the row as it was read", async () => {
      await renderTab();
      await editCell("alice", "alicia");
      fireEvent.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(applyEdits).toHaveBeenCalled());
      expect(applyEdits.mock.calls[0][3]).toEqual([
        {
          kind: "update",
          row: [
            ["id", "1"],
            ["name", "alice"],
          ],
          set: [["name", "alicia"]],
        },
      ]);
    });

    it("confirms before writing, naming the table", async () => {
      await renderTab();
      await editCell("alice", "alicia");
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(confirm).toHaveBeenCalled());
      expect(String(confirm.mock.calls[0][0])).toContain("users");
    });

    it("saves nothing when the confirmation is declined", async () => {
      confirm.mockResolvedValueOnce(false);
      await renderTab();
      await editCell("alice", "alicia");
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(confirm).toHaveBeenCalled());
      expect(applyEdits).not.toHaveBeenCalled();
    });

    /** Otherwise Save stays lit with a no-op UPDATE behind it. */
    it("stops offering Save once a cell is put back to what it was", async () => {
      await renderTab();
      await editCell("alice", "alicia");
      await editCell("alicia", "alice");
      await waitFor(() =>
        expect(screen.queryByRole("button", { name: "Save" })).toBeNull(),
      );
    });

    it("stages a delete as a delete, not by hiding the row", async () => {
      await renderTab();
      fireEvent.click(screen.getAllByLabelText("Delete this row")[0]);
      // Still on screen, so it can be taken back.
      expect(screen.getByText("alice")).toBeTruthy();

      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(applyEdits).toHaveBeenCalled());
      expect(applyEdits.mock.calls[0][3][0]).toMatchObject({ kind: "delete" });
    });
  });

  describe("filters", () => {
    it("refetches once a condition is complete, not while it is half typed", async () => {
      await renderTab();
      const initial = fetchTableRows.mock.calls.length;

      fireEvent.click(screen.getByRole("button", { name: /Add a filter/ }));
      // A column is chosen but no value yet — nothing to ask the server.
      await waitFor(() =>
        expect(screen.getByLabelText("Filter value")).toBeTruthy(),
      );
      expect(fetchTableRows.mock.calls.length).toBe(initial);

      fireEvent.change(screen.getByLabelText("Filter value"), {
        target: { value: "alice" },
      });
      await waitFor(() =>
        expect(fetchTableRows.mock.calls.length).toBeGreaterThan(initial),
      );
    });
  });

  describe("the column manager", () => {
    it("hides a column from the grid without losing it from the row", async () => {
      await renderTab();
      expect(screen.getByRole("columnheader", { name: "name" })).toBeTruthy();

      fireEvent.click(screen.getByLabelText(/^Columns \(/));
      fireEvent.click(await screen.findByLabelText("name"));
      await waitFor(() =>
        expect(screen.queryByRole("columnheader", { name: "name" })).toBeNull(),
      );

      // The hidden column is still part of the row that gets saved.
      fireEvent.click(screen.getAllByLabelText("Delete this row")[0]);
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(applyEdits).toHaveBeenCalled());
      const [edit] = applyEdits.mock.calls[0][3];
      expect(edit).toMatchObject({
        kind: "delete",
        row: [
          ["id", "1"],
          ["name", "alice"],
        ],
      });
    });
  });

  describe("selection", () => {
    it("stages a delete for every ticked row at once", async () => {
      await renderTab();
      fireEvent.click(
        screen.getByLabelText("Select all rows on this page"),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: /Delete 2 selected/ }),
      );

      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(applyEdits).toHaveBeenCalled());
      const edits = applyEdits.mock.calls[0][3];
      expect(edits).toHaveLength(2);
      expect(edits.every((e) => e.kind === "delete")).toBe(true);
    });
  });

  describe("the quick filter", () => {
    it("narrows the loaded page without asking the server again", async () => {
      await renderTab();
      const before = fetchTableRows.mock.calls.length;

      fireEvent.change(screen.getByLabelText("Filter the rows on this page"), {
        target: { value: "bob" },
      });
      await waitFor(() => expect(screen.queryByText("alice")).toBeNull());
      expect(screen.getByText("bob")).toBeTruthy();
      expect(fetchTableRows.mock.calls.length).toBe(before);
    });
  });

  describe("following a relationship", () => {
    it("opens the table a foreign key points at, filtered to that row", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");

      fireEvent.click((await screen.findAllByTitle(/View in users/))[0]);
      expect(navigate).toHaveBeenCalledWith({
        table: "users",
        filters: [{ column: "id", op: "eq", value: "7" }],
      });
    });

    it("opens the rows that point back, filtered to their parent", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");

      fireEvent.click((await screen.findAllByTitle("View related tasks"))[0]);
      expect(navigate).toHaveBeenCalledWith({
        table: "tasks",
        filters: [{ column: "project_id", op: "eq", value: "1" }],
      });
    });

    it("shows how many rows point back", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");
      const counts = (await screen.findAllByTitle("View related tasks")).map(
        (chip) => chip.textContent,
      );
      expect(counts[0]).toContain("3");
      expect(counts[1]).toContain("0");
    });

    /** The count column is the backend's, not the table's. */
    it("never draws __rel_0 as a column of its own", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");
      expect(screen.queryByRole("columnheader", { name: "__rel_0" })).toBeNull();
    });
  });

  describe("the row drawer", () => {
    it("opens a row as a form and stages its edits like the grid does", async () => {
      await renderTab();
      fireEvent.click(screen.getAllByLabelText("Open this row")[0]);

      const field = await screen.findByLabelText("name");
      fireEvent.change(field, { target: { value: "alicia" } });
      fireEvent.click(screen.getByRole("button", { name: "Done" }));

      fireEvent.click(await screen.findByRole("button", { name: "Save" }));
      await waitFor(() => expect(applyEdits).toHaveBeenCalled());
      expect(applyEdits.mock.calls[0][3][0]).toMatchObject({
        kind: "update",
        set: [["name", "alicia"]],
      });
    });

    it("says a row cannot be edited on a read-only connection", async () => {
      await renderTab({ writable: false });
      fireEvent.click(screen.getAllByLabelText("Open this row")[0]);
      expect(
        await screen.findByText(/can't be edited/i),
      ).toBeTruthy();
    });
  });

  describe("reordering columns", () => {
    const headers = () =>
      screen
        .getAllByRole("columnheader")
        .map((h) => h.textContent?.trim())
        .filter((t) => t && t !== "");

    it("moves one along from the column manager", async () => {
      await renderTab();
      expect(headers()).toEqual(["id", "name"]);

      fireEvent.click(screen.getByLabelText(/^Columns \(/));
      fireEvent.click(await screen.findByLabelText("Move name left"));
      await waitFor(() => expect(headers()).toEqual(["name", "id"]));
    });

    /** Nothing to the left of the first column, nothing right of the last. */
    it("does not offer a move off either end", async () => {
      await renderTab();
      fireEvent.click(screen.getByLabelText(/^Columns \(/));
      expect(await screen.findByLabelText("Move id left")).toHaveProperty(
        "disabled",
        true,
      );
      expect(screen.getByLabelText("Move name right")).toHaveProperty(
        "disabled",
        true,
      );
    });

    it("moves one by dragging its header onto another", async () => {
      await renderTab();
      // By name, not by position: the selection and actions columns are
      // headers too, and the first one is not "id".
      const id = screen.getByRole("columnheader", { name: "id" });
      const name = screen.getByRole("columnheader", { name: "name" });

      const data = new Map<string, string>();
      const dataTransfer = {
        effectAllowed: "",
        dropEffect: "",
        setData: (k: string, v: string) => data.set(k, v),
        getData: (k: string) => data.get(k) ?? "",
      };
      fireEvent.dragStart(id, { dataTransfer });
      fireEvent.dragOver(name, { dataTransfer });
      fireEvent.drop(name, { dataTransfer });

      await waitFor(() => expect(headers()).toEqual(["name", "id"]));
    });

    /** The order outlives the page it was set on. */
    it("keeps the order when the rows are reloaded", async () => {
      await renderTab();
      fireEvent.click(screen.getByLabelText(/^Columns \(/));
      fireEvent.click(await screen.findByLabelText("Move name left"));
      await waitFor(() => expect(headers()).toEqual(["name", "id"]));

      fireEvent.click(screen.getByLabelText("Reload rows"));
      await waitFor(() => expect(headers()).toEqual(["name", "id"]));
    });
  });

  describe("related tables as columns", () => {
    const openManager = async () => {
      fireEvent.click(screen.getByLabelText(/^Columns \(/));
      return screen.findByRole("dialog", { name: "Column manager" });
    };

    it("lists them alongside the table's own columns", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");
      const manager = await openManager();

      expect(within(manager).getByLabelText("id")).toBeTruthy();
      expect(within(manager).getByLabelText("tasks")).toBeTruthy();
    });

    it("hides one from the grid", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");
      expect(screen.getAllByTitle("View related tasks").length).toBeGreaterThan(0);

      const manager = await openManager();
      fireEvent.click(within(manager).getByLabelText("tasks"));
      await waitFor(() =>
        expect(screen.queryByTitle("View related tasks")).toBeNull(),
      );
    });

    /** A relation to `users` must not be hidden by hiding a `users` column. */
    it("keeps a relation apart from a column of the same name", async () => {
      const sameName: ResultSet = {
        columns: [
          { name: "id", type: "int" },
          { name: "tasks", type: "text" },
          { name: "__rel_0", type: "" },
        ],
        rows: [["1", "a note", "3"]],
        total: 1,
      };
      fetchTableRows.mockResolvedValue(sameName);
      await renderTab(
        {
          structure: {
            ...LINKED_STRUCTURE,
            columns: ["id", "tasks"].map((name) => ({
              name,
              type: "text",
              nullable: false,
              default: "",
              primary_key: name === "id",
              extra: "",
            })),
            foreign_keys: [],
          },
        },
        "a note",
      );

      const manager = await openManager();
      // Two rows called "tasks": the column and the relation.
      expect(within(manager).getAllByLabelText("tasks")).toHaveLength(2);

      // Hiding the first — the table's own column — leaves the relation up.
      fireEvent.click(within(manager).getAllByLabelText("tasks")[0]);
      await waitFor(() => expect(screen.queryByText("a note")).toBeNull());
      expect(screen.getByTitle("View related tasks")).toBeTruthy();
    });

    it("moves one in among the table's own columns", async () => {
      fetchTableRows.mockResolvedValue(LINKED);
      await renderTab({ structure: LINKED_STRUCTURE }, "7");
      const manager = await openManager();

      // Relations start after the table's columns; walk one to the front.
      fireEvent.click(within(manager).getByLabelText("Move tasks left"));
      fireEvent.click(within(manager).getByLabelText("Move tasks left"));
      fireEvent.click(within(manager).getByLabelText("Move tasks left"));

      // The property, not a fixed index: the relation now sits ahead of the
      // table's own columns rather than after them.
      await waitFor(() => {
        const headers = screen
          .getAllByRole("columnheader")
          .map((h) => h.textContent?.trim())
          .filter(Boolean) as string[];
        expect(headers.indexOf("tasks")).toBeLessThan(headers.indexOf("id"));
      });
    });
  });
});
