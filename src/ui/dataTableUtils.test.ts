import { describe, expect, it } from "vitest";
import { nextSort, sortRows } from "./dataTableUtils";
import type { Column } from "./DataTable";

interface Row {
  name: string;
  port: number;
}

const columns: Column<Row>[] = [
  { key: "name", header: "Name", render: (r) => r.name, sortValue: (r) => r.name },
  { key: "port", header: "Port", render: (r) => r.port, sortValue: (r) => r.port },
  // Deliberately has no sortValue — not sortable.
  { key: "actions", header: "", render: () => null },
];

const rows: Row[] = [
  { name: "web", port: 8080 },
  { name: "api", port: 443 },
  { name: "db", port: 5432 },
];

describe("sortRows", () => {
  it("returns rows untouched when no sort is given", () => {
    expect(sortRows(rows, columns, undefined)).toBe(rows);
  });

  it("sorts strings ascending and descending", () => {
    expect(sortRows(rows, columns, { key: "name", dir: "asc" }).map((r) => r.name)).toEqual([
      "api",
      "db",
      "web",
    ]);
    expect(sortRows(rows, columns, { key: "name", dir: "desc" }).map((r) => r.name)).toEqual([
      "web",
      "db",
      "api",
    ]);
  });

  it("sorts numbers numerically, not lexically", () => {
    // A lexical sort would put 443 after 5432 and 8080.
    expect(sortRows(rows, columns, { key: "port", dir: "asc" }).map((r) => r.port)).toEqual([
      443, 5432, 8080,
    ]);
  });

  it("orders embedded numbers naturally", () => {
    const natural: Row[] = [
      { name: "worker10", port: 1 },
      { name: "worker2", port: 2 },
      { name: "worker1", port: 3 },
    ];
    expect(
      sortRows(natural, columns, { key: "name", dir: "asc" }).map((r) => r.name),
    ).toEqual(["worker1", "worker2", "worker10"]);
  });

  it("is case-insensitive", () => {
    const mixed: Row[] = [
      { name: "Zeta", port: 1 },
      { name: "alpha", port: 2 },
    ];
    expect(sortRows(mixed, columns, { key: "name", dir: "asc" }).map((r) => r.name)).toEqual([
      "alpha",
      "Zeta",
    ]);
  });

  it("ignores a sort key that is missing or not sortable", () => {
    // A stale persisted sort key must not throw or reorder arbitrarily.
    expect(sortRows(rows, columns, { key: "actions", dir: "asc" })).toBe(rows);
    expect(sortRows(rows, columns, { key: "nope", dir: "asc" })).toBe(rows);
  });

  it("does not mutate the input array", () => {
    const original = [...rows];
    sortRows(rows, columns, { key: "name", dir: "asc" });
    expect(rows).toEqual(original);
  });

  it("pushes NaN to the end rather than corrupting the sort", () => {
    const withNaN: Row[] = [
      { name: "a", port: Number.NaN },
      { name: "b", port: 2 },
      { name: "c", port: 1 },
    ];
    expect(
      sortRows(withNaN, columns, { key: "port", dir: "asc" }).map((r) => r.name),
    ).toEqual(["c", "b", "a"]);
  });
});

describe("nextSort", () => {
  it("starts a new column ascending", () => {
    expect(nextSort(undefined, "name")).toEqual({ key: "name", dir: "asc" });
    expect(nextSort({ key: "port", dir: "desc" }, "name")).toEqual({
      key: "name",
      dir: "asc",
    });
  });

  it("flips direction on the active column", () => {
    expect(nextSort({ key: "name", dir: "asc" }, "name")).toEqual({
      key: "name",
      dir: "desc",
    });
    expect(nextSort({ key: "name", dir: "desc" }, "name")).toEqual({
      key: "name",
      dir: "asc",
    });
  });
});
