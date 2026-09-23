import type { Filter, FilterOp } from "../api";

/**
 * The filter bar's vocabulary. The operator list is fixed and mirrors the
 * backend's own enum — an operator becomes SQL syntax, so it is chosen from a
 * list, never typed.
 */
export interface OpDefinition {
  value: FilterOp;
  label: string;
  /** IS NULL / IS NOT NULL take no operand; the value box is hidden for them. */
  takesValue: boolean;
}

export const FILTER_OPS: OpDefinition[] = [
  { value: "eq", label: "is", takesValue: true },
  { value: "ne", label: "is not", takesValue: true },
  { value: "contains", label: "contains", takesValue: true },
  { value: "not_contains", label: "does not contain", takesValue: true },
  { value: "starts_with", label: "starts with", takesValue: true },
  { value: "ends_with", label: "ends with", takesValue: true },
  { value: "gt", label: ">", takesValue: true },
  { value: "gte", label: "≥", takesValue: true },
  { value: "lt", label: "<", takesValue: true },
  { value: "lte", label: "≤", takesValue: true },
  { value: "is_null", label: "is NULL", takesValue: false },
  { value: "not_null", label: "is not NULL", takesValue: false },
];

export const opTakesValue = (op: FilterOp): boolean =>
  FILTER_OPS.find((o) => o.value === op)?.takesValue ?? true;

/** A filter row as the bar holds it, before it is known to be complete. */
export interface DraftFilter extends Filter {
  id: string;
}

let seq = 0;
export function newFilter(column: string): DraftFilter {
  seq += 1;
  return { id: `f${seq}`, column, op: "eq", value: "" };
}

/**
 * The filters worth sending: a column must be chosen, and an operator that
 * takes an operand must have one. Half-typed rows are dropped rather than
 * turned into `WHERE col = ''`, which would silently show nothing.
 */
export function activeFilters(drafts: DraftFilter[]): Filter[] {
  return drafts
    .filter((f) => f.column !== "" && (!opTakesValue(f.op) || f.value !== ""))
    .map(({ column, op, value }) => ({ column, op, value }));
}

/** Stable key for the row fetch, so an incomplete edit doesn't refetch. */
export const filterKey = (filters: Filter[]): string => JSON.stringify(filters);
