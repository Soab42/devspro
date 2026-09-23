import { Button, IconButton, Select, Stack, TextInput } from "../../../ui";
import { Funnel, Plus, X } from "../../../ui/icons";
import type { FilterOp } from "../api";
import { DraftFilter, FILTER_OPS, newFilter, opTakesValue } from "./filters";

export interface FilterBarProps {
  columns: string[];
  filters: DraftFilter[];
  onChange: (filters: DraftFilter[]) => void;
  disabled?: boolean;
}

export interface AddFilterButtonProps {
  columns: string[];
  filters: DraftFilter[];
  onChange: (filters: DraftFilter[]) => void;
  disabled?: boolean;
}

/**
 * Adds a condition row. Separate from the bar so the Data tab can put it in
 * its toolbar, on the same line as the search box, instead of on a line of
 * its own under a bar that is usually empty.
 */
export function AddFilterButton({
  columns,
  filters,
  onChange,
  disabled,
}: AddFilterButtonProps) {
  return (
    <Button
      size="sm"
      variant="secondary"
      icon={filters.length === 0 ? Funnel : Plus}
      disabled={disabled || columns.length === 0}
      onClick={() => onChange([...filters, newFilter(columns[0] ?? "")])}
    >
      {filters.length === 0 ? "Add a filter" : "Add condition"}
    </Button>
  );
}

/**
 * Conditions ANDed together, one per row. Renders nothing until there is one;
 * `AddFilterButton` is what puts the first row here.
 *
 * Rows apply as they are completed rather than behind an Apply button: an
 * incomplete row is simply ignored (see `activeFilters`), so there is no state
 * where the grid and the bar disagree about what is being shown.
 */
export default function FilterBar({
  columns,
  filters,
  onChange,
  disabled,
}: FilterBarProps) {
  const columnOptions = columns.map((c) => ({ value: c, label: c }));
  const opOptions = FILTER_OPS.map((o) => ({ value: o.value, label: o.label }));

  function update(id: string, patch: Partial<DraftFilter>) {
    onChange(filters.map((f) => (f.id === id ? { ...f, ...patch } : f)));
  }

  if (filters.length === 0) return null;

  return (
    <Stack gap={2} className="db-filter-bar">
      {filters.map((filter) => (
        <div className="db-filter-row" key={filter.id}>
          <Select
            aria-label="Filter column"
            size="sm"
            value={filter.column}
            options={columnOptions}
            searchable
            searchPlaceholder="Search columns…"
            placeholder="Column"
            disabled={disabled}
            onChange={(column) => update(filter.id, { column })}
          />
          <Select
            aria-label="Filter operator"
            size="sm"
            value={filter.op}
            options={opOptions}
            disabled={disabled}
            onChange={(op) => update(filter.id, { op: op as FilterOp })}
          />
          {opTakesValue(filter.op) ? (
            <TextInput
              inputSize="sm"
              mono
              aria-label="Filter value"
              value={filter.value}
              disabled={disabled}
              placeholder="Value"
              onChange={(e) => update(filter.id, { value: e.target.value })}
            />
          ) : (
            <span aria-hidden="true" />
          )}
          <IconButton
            icon={X}
            label="Remove this condition"
            size="sm"
            variant="ghost"
            disabled={disabled}
            onClick={() => onChange(filters.filter((f) => f.id !== filter.id))}
          />
        </div>
      ))}
    </Stack>
  );
}
