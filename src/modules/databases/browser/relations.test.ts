import { describe, expect, it } from "vitest";
import type { ResultSet, TableStructure } from "../api";
import {
  isRelationKey,
  linkFilter,
  outgoingLinks,
  relationColumns,
  relationFilter,
  relationIndexOf,
  relationKey,
  relationsByKey,
  splitResult,
} from "./relations";

const structure = (over: Partial<TableStructure> = {}): TableStructure => ({
  columns: [],
  indexes: [],
  foreign_keys: [],
  referenced_by: [],
  ...over,
});

const fk = (name: string, columns: string[], table: string, refs: string[]) => ({
  name,
  columns,
  references_table: table,
  references_columns: refs,
  on_delete: "",
  on_update: "",
});

describe("outgoingLinks", () => {
  it("makes the key column a link to the row it points at", () => {
    const links = outgoingLinks(
      structure({ foreign_keys: [fk("fk1", ["user_id"], "users", ["id"])] }),
    );
    expect(links.get("user_id")).toEqual({
      column: "user_id",
      table: "users",
      referencesColumn: "id",
    });
  });

  /** No one cell is the link when the key is spread across several. */
  it("leaves a composite key alone", () => {
    const links = outgoingLinks(
      structure({
        foreign_keys: [fk("fk1", ["order_id", "line"], "lines", ["id", "no"])],
      }),
    );
    expect(links.size).toBe(0);
  });

  it("copes with a structure that hasn't loaded yet", () => {
    expect(outgoingLinks(null).size).toBe(0);
  });
});

describe("relationColumns", () => {
  const relation = (name: string, table: string, columns: string[]) => ({
    name,
    table,
    columns,
    references_columns: ["id"],
  });

  it("gives one column per table that points back", () => {
    const relations = relationColumns(
      structure({
        referenced_by: [
          relation("a", "tasks", ["project_id"]),
          relation("b", "sprints", ["project_id"]),
        ],
      }),
    );
    expect(relations.map((r) => r.table)).toEqual(["tasks", "sprints"]);
  });

  /** Two constraints on the same columns would draw two identical columns. */
  it("deduplicates by table and key", () => {
    const relations = relationColumns(
      structure({
        referenced_by: [
          relation("a", "tasks", ["project_id"]),
          relation("b", "tasks", ["project_id"]),
          relation("c", "tasks", ["owner_id"]),
        ],
      }),
    );
    expect(relations).toHaveLength(2);
  });
});

describe("splitResult", () => {
  it("keeps the table's own columns apart from the appended counts", () => {
    const rs: ResultSet = {
      columns: [
        { name: "id", type: "" },
        { name: "name", type: "" },
        { name: "__rel_0", type: "" },
        { name: "__rel_1", type: "" },
      ],
      rows: [["1", "x", "3", "0"]],
      total: 1,
    };
    const { dataColumns, relationIndexes } = splitResult(rs);
    expect(dataColumns.map((c) => c.name)).toEqual(["id", "name"]);
    expect(relationIndexes).toEqual([2, 3]);
  });

  it("recognises a relation column by its index, not just its prefix", () => {
    expect(relationIndexOf("__rel_0")).toBe(0);
    expect(relationIndexOf("__rel_12")).toBe(12);
    expect(relationIndexOf("__rel_x")).toBeNull();
    expect(relationIndexOf("id")).toBeNull();
  });
});

describe("navigation filters", () => {
  const link = { column: "user_id", table: "users", referencesColumn: "id" };

  it("opens the referenced row by its key", () => {
    expect(linkFilter(link, "7")).toEqual([
      { column: "id", op: "eq", value: "7" },
    ]);
  });

  /** A NULL key points nowhere — following it would list every NULL row. */
  it("is not a link when the key is NULL", () => {
    expect(linkFilter(link, null)).toBeNull();
  });

  it("opens a relation's children by the key they point at", () => {
    const relation = {
      table: "tasks",
      columns: ["project_id"],
      references_columns: ["id"],
    };
    expect(relationFilter(relation, { id: "3" })).toEqual([
      { column: "project_id", op: "eq", value: "3" },
    ]);
  });

  it("refuses a composite relation with a missing half", () => {
    const relation = {
      table: "lines",
      columns: ["order_id", "line"],
      references_columns: ["id", "no"],
    };
    expect(relationFilter(relation, { id: "1", no: null })).toBeNull();
    expect(relationFilter(relation, { id: "1", no: "2" })).toHaveLength(2);
  });
});

describe("relation column keys", () => {
  const spec = {
    table: "users",
    columns: ["owner_id"],
    references_columns: ["id"],
  };

  /** Otherwise hiding the `users` column would hide the `users` relation. */
  it("cannot collide with a table column of the same name", () => {
    expect(relationKey(spec)).not.toBe("users");
    expect(isRelationKey(relationKey(spec))).toBe(true);
    expect(isRelationKey("users")).toBe(false);
  });

  it("distinguishes two relations to the same table on different keys", () => {
    const other = { ...spec, columns: ["reviewer_id"] };
    expect(relationKey(spec)).not.toBe(relationKey(other));
  });

  it("looks a relation up by key, with the position its count column sits at", () => {
    const other = { ...spec, table: "tasks" };
    const map = relationsByKey([spec, other]);
    expect(map.get(relationKey(spec))).toEqual({ relation: spec, index: 0 });
    expect(map.get(relationKey(other))).toEqual({ relation: other, index: 1 });
  });
});
