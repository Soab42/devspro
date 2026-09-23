import { describe, expect, it } from "vitest";
import {
  activeFilters,
  FILTER_OPS,
  filterKey,
  newFilter,
  opTakesValue,
} from "./filters";

describe("filter operators", () => {
  it("knows which operators take an operand", () => {
    expect(opTakesValue("eq")).toBe(true);
    expect(opTakesValue("is_null")).toBe(false);
    expect(opTakesValue("not_null")).toBe(false);
  });

  it("offers every operator with a label", () => {
    expect(FILTER_OPS.every((o) => o.label.length > 0)).toBe(true);
  });
});

describe("activeFilters", () => {
  /** Otherwise a half-typed row becomes `WHERE col = ''` and shows nothing. */
  it("drops a row whose value is still empty", () => {
    const draft = { ...newFilter("status"), value: "" };
    expect(activeFilters([draft])).toEqual([]);
  });

  it("drops a row with no column chosen", () => {
    const draft = { ...newFilter(""), value: "x" };
    expect(activeFilters([draft])).toEqual([]);
  });

  it("keeps a NULL check even though it has no value", () => {
    const draft = { ...newFilter("deleted_at"), op: "is_null" as const };
    expect(activeFilters([draft])).toEqual([
      { column: "deleted_at", op: "is_null", value: "" },
    ]);
  });

  it("strips the draft id before sending", () => {
    const draft = { ...newFilter("name"), value: "alice" };
    expect(activeFilters([draft])[0]).not.toHaveProperty("id");
  });
});

describe("filterKey", () => {
  it("is stable for the same filters, so editing a draft doesn't refetch", () => {
    const a = [{ ...newFilter("name"), value: "x" }];
    const b = [{ ...newFilter("name"), value: "x" }];
    expect(filterKey(activeFilters(a))).toBe(filterKey(activeFilters(b)));
  });
});
