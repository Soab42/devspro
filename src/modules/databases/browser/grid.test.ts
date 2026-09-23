import { describe, expect, it } from "vitest";
import type { ResultSet } from "../api";
import { isRowChanged, rowsFromResult, setCell } from "./edits";
import {
  clearedColumns,
  filterRows,
  headerTick,
  rowMatches,
  moveColumn,
  orderedColumns,
  reorderColumn,
  toggleAll,
  toggleHidden,
  toggleOne,
  visibleColumns,
} from "./grid";

const COLUMNS = ["id", "name", "note"];

const rows = () =>
  rowsFromResult({
    columns: COLUMNS.map((name) => ({ name, type: "" })),
    rows: [
      ["1", "alice", "NULL"],
      ["2", "bob", "hi"],
      ["3", "carol", "there"],
    ],
    total: 3,
  } as ResultSet);

describe("column visibility", () => {
  it("hides what is ticked off, keeping the table's own order", () => {
    const hidden = toggleHidden(new Set<string>(), "name");
    expect(visibleColumns(COLUMNS, hidden)).toEqual(["id", "note"]);
  });

  it("toggles back on", () => {
    let hidden = toggleHidden(new Set<string>(), "name");
    hidden = toggleHidden(hidden, "name");
    expect(visibleColumns(COLUMNS, hidden)).toEqual(COLUMNS);
  });

  /**
   * Storing what is *hidden* rather than what is visible means a column added
   * to the table later shows up instead of silently missing.
   */
  it("shows a column that did not exist when the choice was made", () => {
    const hidden = toggleHidden(new Set<string>(), "note");
    expect(visibleColumns([...COLUMNS, "added_later"], hidden)).toContain(
      "added_later",
    );
  });

  /** A grid with no columns has no way back to one. */
  it("always leaves one column when everything is cleared", () => {
    expect(visibleColumns(COLUMNS, clearedColumns(COLUMNS))).toEqual(["id"]);
  });
});

describe("the quick filter", () => {
  it("matches any cell, case-insensitively", () => {
    const [alice] = rows();
    expect(rowMatches(alice, COLUMNS, "ALI")).toBe(true);
    expect(rowMatches(alice, COLUMNS, "nothing")).toBe(false);
  });

  it("can find NULLs by name", () => {
    const [alice, bob] = rows();
    expect(rowMatches(alice, COLUMNS, "null")).toBe(true);
    expect(rowMatches(bob, COLUMNS, "null")).toBe(false);
  });

  it("narrows the loaded rows", () => {
    expect(filterRows(rows(), COLUMNS, "bob", isRowChanged)).toHaveLength(1);
    expect(filterRows(rows(), COLUMNS, "", isRowChanged)).toHaveLength(3);
  });

  /** Otherwise an edit can be hidden behind a filter and lost on Save. */
  it("keeps a row with unsaved changes visible even when it no longer matches", () => {
    const [alice, ...rest] = rows();
    const edited = setCell(alice, "name", "zzz");
    const shown = filterRows([edited, ...rest], COLUMNS, "bob", isRowChanged);
    expect(shown.map((r) => r.id)).toContain(edited.id);
  });
});

describe("row selection", () => {
  it("ticks and unticks one row", () => {
    const one = toggleOne(new Set<string>(), "r0");
    expect(one.has("r0")).toBe(true);
    expect(toggleOne(one, "r0").has("r0")).toBe(false);
  });

  /** Select-all means the rows you can see, not the ones a filter removed. */
  it("selects only what is on screen", () => {
    const visible = rows().slice(0, 2);
    const selected = toggleAll(new Set<string>(), visible);
    expect([...selected].sort()).toEqual(["r0", "r1"]);
  });

  it("unticks everything when all visible rows are already ticked", () => {
    const visible = rows();
    const all = toggleAll(new Set<string>(), visible);
    expect(toggleAll(all, visible).size).toBe(0);
  });

  it("reports a partial selection so the header box can show it", () => {
    const visible = rows();
    expect(headerTick(new Set(), visible)).toBe("none");
    expect(headerTick(new Set(["r0"]), visible)).toBe("some");
    expect(headerTick(new Set(["r0", "r1", "r2"]), visible)).toBe("all");
    expect(headerTick(new Set(["r0"]), [])).toBe("none");
  });
});

describe("column order", () => {
  it("is the table's own until something is moved", () => {
    expect(orderedColumns(COLUMNS, [])).toEqual(COLUMNS);
  });

  it("moves a column one place either way", () => {
    expect(moveColumn(COLUMNS, [], "note", -1)).toEqual(["id", "note", "name"]);
    expect(moveColumn(COLUMNS, [], "id", 1)).toEqual(["name", "id", "note"]);
  });

  it("stays put at either end rather than wrapping around", () => {
    expect(moveColumn(COLUMNS, [], "id", -1)).toEqual(COLUMNS);
    expect(moveColumn(COLUMNS, [], "note", 1)).toEqual(COLUMNS);
  });

  /**
   * Which side of the drop target the column lands on follows the direction
   * of travel. Always inserting before it would mean a column dragged to the
   * far right could never become the last one.
   */
  it("drops past the target when dragged right, before it when dragged left", () => {
    expect(reorderColumn(COLUMNS, [], "id", "note")).toEqual([
      "name",
      "note",
      "id",
    ]);
    expect(reorderColumn(COLUMNS, [], "note", "id")).toEqual([
      "note",
      "id",
      "name",
    ]);
    expect(reorderColumn(COLUMNS, [], "id", "id")).toEqual(COLUMNS);
  });

  /**
   * The order is a preference, not a schema — it outlives the table it was
   * made against, and has to cope with the table having changed underneath.
   */
  it("appends a column the order has never heard of", () => {
    const order = ["note", "name", "id"];
    expect(orderedColumns([...COLUMNS, "added_later"], order)).toEqual([
      "note",
      "name",
      "id",
      "added_later",
    ]);
  });

  it("ignores a column the order names that no longer exists", () => {
    expect(orderedColumns(COLUMNS, ["dropped", "note"])).toEqual([
      "note",
      "id",
      "name",
    ]);
  });

  it("draws in the chosen order, still minus what is hidden", () => {
    const hidden = toggleHidden(new Set<string>(), "name");
    expect(visibleColumns(COLUMNS, hidden, ["note", "name", "id"])).toEqual([
      "note",
      "id",
    ]);
  });

  /**
   * Moving happens in the full order, hidden columns included: moving past a
   * hidden one would otherwise look like nothing happened, and un-hiding it
   * later would put things somewhere never asked for.
   */
  it("counts hidden columns when moving", () => {
    const moved = moveColumn(COLUMNS, [], "note", -1);
    expect(moved).toEqual(["id", "note", "name"]);
    expect(visibleColumns(COLUMNS, new Set(["note"]), moved)).toEqual([
      "id",
      "name",
    ]);
  });
});
