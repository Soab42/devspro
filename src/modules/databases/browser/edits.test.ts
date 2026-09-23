import { describe, expect, it } from "vitest";
import type { ResultSet, TableStructure } from "../api";
import {
  blankRow,
  cellValue,
  changeCount,
  editability,
  isRowChanged,
  pendingEdits,
  revertRow,
  rowsFromResult,
  setCell,
  toggleDeleted,
} from "./edits";

const COLUMNS = ["id", "name", "note"];

function result(): ResultSet {
  return {
    columns: COLUMNS.map((name) => ({ name, type: "" })),
    rows: [
      ["1", "alice", "NULL"],
      ["2", "bob", "hi"],
    ],
    total: 2,
  };
}

const structure = (primary: string[]): TableStructure => ({
  columns: COLUMNS.map((name) => ({
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

describe("reading rows", () => {
  it("turns the NULL marker into an actual null", () => {
    const rows = rowsFromResult(result());
    expect(cellValue(rows[0], "note")).toBeNull();
    expect(cellValue(rows[1], "note")).toBe("hi");
  });

  it("starts with nothing staged", () => {
    expect(changeCount(rowsFromResult(result()))).toBe(0);
  });
});

describe("staging a change", () => {
  it("shows the edit in place of what was read", () => {
    const row = setCell(rowsFromResult(result())[0], "name", "alicia");
    expect(cellValue(row, "name")).toBe("alicia");
    expect(isRowChanged(row)).toBe(true);
  });

  /** Otherwise the Save button stays lit with a no-op UPDATE behind it. */
  it("drops the change when a cell is put back to what it was", () => {
    let row = rowsFromResult(result())[0];
    row = setCell(row, "name", "alicia");
    row = setCell(row, "name", "alice");
    expect(isRowChanged(row)).toBe(false);
    expect(pendingEdits([row], COLUMNS)).toEqual([]);
  });

  it("treats setting a cell to NULL as a real change", () => {
    const row = setCell(rowsFromResult(result())[1], "note", null);
    expect(cellValue(row, "note")).toBeNull();
    expect(pendingEdits([row], COLUMNS)).toEqual([
      {
        kind: "update",
        row: [
          ["id", "2"],
          ["name", "bob"],
          ["note", "hi"],
        ],
        set: [["note", null]],
      },
    ]);
  });

  it("sends only the cells that changed, not the whole row", () => {
    const row = setCell(rowsFromResult(result())[0], "name", "alicia");
    const [edit] = pendingEdits([row], COLUMNS);
    expect(edit).toMatchObject({ kind: "update", set: [["name", "alicia"]] });
  });
});

describe("deleting and reverting", () => {
  it("stages a delete against the row as it was read", () => {
    const row = toggleDeleted(rowsFromResult(result())[0])!;
    expect(pendingEdits([row], COLUMNS)).toEqual([
      {
        kind: "delete",
        row: [
          ["id", "1"],
          ["name", "alice"],
          ["note", null],
        ],
      },
    ]);
  });

  it("discards a new row outright instead of staging a delete for it", () => {
    expect(toggleDeleted(blankRow())).toBeNull();
    expect(revertRow(blankRow())).toBeNull();
  });

  it("reverting clears both the edits and a pending delete", () => {
    let row = setCell(rowsFromResult(result())[0], "name", "x");
    row = toggleDeleted(row)!;
    expect(isRowChanged(revertRow(row)!)).toBe(false);
  });
});

describe("new rows", () => {
  it("ignores a blank new row so Add row doesn't insert an empty one", () => {
    expect(pendingEdits([blankRow()], COLUMNS)).toEqual([]);
  });

  it("inserts only the columns that were filled in", () => {
    const row = setCell(blankRow(), "name", "carol");
    expect(pendingEdits([row], COLUMNS)).toEqual([
      { kind: "insert", values: [["name", "carol"]] },
    ]);
  });
});

describe("ordering", () => {
  /** Applied in the order a person would expect to read them. */
  it("puts inserts first, then updates, then deletes", () => {
    const rows = rowsFromResult(result());
    const staged = [
      toggleDeleted(rows[0])!,
      setCell(rows[1], "name", "bobby"),
      setCell(blankRow(), "name", "carol"),
    ];
    expect(pendingEdits(staged, COLUMNS).map((e) => e.kind)).toEqual([
      "insert",
      "update",
      "delete",
    ]);
  });
});

describe("editability", () => {
  it("refuses a view", () => {
    const { editable, reason } = editability(structure(["id"]), "view");
    expect(editable).toBe(false);
    expect(reason).toMatch(/Views/);
  });

  it("refuses a table with no primary key, and says why", () => {
    const { editable, reason } = editability(structure([]), "table");
    expect(editable).toBe(false);
    expect(reason).toMatch(/no primary key/);
  });

  it("allows a table with one", () => {
    expect(editability(structure(["id"]), "table").editable).toBe(true);
  });

  it("says nothing while the structure is still loading", () => {
    expect(editability(null, "table")).toEqual({
      editable: false,
      reason: null,
    });
  });
});
