import { useMemo, useState } from "react";
import {
  Badge,
  Button,
  Callout,
  Field,
  Modal,
  ModalFooter,
  Stack,
  Switch,
  TextArea,
  TextInput,
} from "../../../ui";
import { ExternalLink, KeyRound } from "../../../ui/icons";
import type { Cell, ColumnMeta, TableStructure } from "../api";
import { cellValue, EditableRow, isNewRow, setCell } from "./edits";
import { outgoingLinks } from "./relations";

export interface RowDrawerProps {
  table: string;
  row: EditableRow;
  structure: TableStructure | null;
  columns: string[];
  editable: boolean;
  onChange: (row: EditableRow) => void;
  onClose: () => void;
  /** Open the row this one's foreign key points at. */
  onFollow: (column: string, value: Cell) => void;
}

/** A long value deserves a box you can actually read it in. */
const isLong = (meta: ColumnMeta | undefined, value: Cell) =>
  (value?.length ?? 0) > 80 ||
  /text|json|jsonb|blob|bytea/i.test(meta?.type ?? "");

/**
 * One row, as a form, beside the grid it came from.
 *
 * The grid is good for scanning and bad for a row of forty columns, half of
 * them truncated — this is the other half of that. Edits go into the same
 * staged model the grid uses, so Save is still one transaction and Discard
 * still takes everything back; the drawer is a second way to reach the same
 * changes, not a second way to write them.
 */
export default function RowDrawer({
  table,
  row,
  structure,
  columns,
  editable,
  onChange,
  onClose,
  onFollow,
}: RowDrawerProps) {
  const [error, setError] = useState<string | null>(null);
  const metaOf = useMemo(() => {
    const map = new Map<string, ColumnMeta>();
    for (const column of structure?.columns ?? []) map.set(column.name, column);
    return map;
  }, [structure]);
  const links = useMemo(() => outgoingLinks(structure), [structure]);

  function update(column: string, value: Cell) {
    setError(null);
    onChange(setCell(row, column, value));
  }

  return (
    <Modal
      size="md"
      placement="drawer"
      title={isNewRow(row) ? "New row" : "Edit row"}
      description={`Table: ${table}`}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={5}>
        {!editable && (
          <Callout tone="info">
            This row can't be edited — the connection is read-only, or the
            table has no primary key to identify it by.
          </Callout>
        )}
        {error && <Callout tone="danger">{error}</Callout>}

        {columns.map((column) => {
          const meta = metaOf.get(column);
          const value = cellValue(row, column);
          const link = links.get(column);
          const isNull = value === null;

          return (
            <Field
              key={column}
              label={column}
              htmlFor={`row-${column}`}
              description={
                <span className="db-drawer-badges">
                  {meta?.primary_key && (
                    <Badge tone="accent" icon={KeyRound}>
                      PK
                    </Badge>
                  )}
                  {link && (
                    <Badge tone="info">{`FK: ${link.table}`}</Badge>
                  )}
                  {meta?.type && <span className="u-muted">{meta.type}</span>}
                </span>
              }
            >
              <Stack gap={2}>
                {isLong(meta, value) ? (
                  <TextArea
                    id={`row-${column}`}
                    mono
                    rows={4}
                    value={value ?? ""}
                    disabled={!editable || isNull}
                    onChange={(e) => update(column, e.target.value)}
                  />
                ) : (
                  <TextInput
                    id={`row-${column}`}
                    mono
                    value={value ?? ""}
                    disabled={!editable || isNull}
                    placeholder={isNull ? "NULL" : undefined}
                    onChange={(e) => update(column, e.target.value)}
                  />
                )}
                <div className="db-drawer-row-actions">
                  {/* NULL is a value here, not an empty box: a nullable text
                      column can hold both and they are not the same row. */}
                  {meta?.nullable !== false && (
                    <Switch
                      checked={isNull}
                      disabled={!editable}
                      onChange={(next) => update(column, next ? null : "")}
                      label="NULL"
                    />
                  )}
                  {link && !isNull && (
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={ExternalLink}
                      onClick={() => onFollow(column, value)}
                    >
                      {`View in ${link.table}`}
                    </Button>
                  )}
                </div>
              </Stack>
            </Field>
          );
        })}
      </Stack>
    </Modal>
  );
}
