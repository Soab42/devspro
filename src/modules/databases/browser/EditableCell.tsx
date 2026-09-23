import { useEffect, useRef, useState } from "react";
import type { Cell } from "../api";

export interface EditableCellProps {
  value: Cell;
  changed: boolean;
  editable: boolean;
  onCommit: (value: Cell) => void;
}

const CELL_MAX = 200;

/**
 * One grid cell: display until clicked, an input while editing.
 *
 * NULL is a first-class value here, not the empty string — a text column can
 * hold both and they are not the same row. Clearing the box therefore commits
 * an empty string, and NULL is set deliberately with Ctrl/Cmd+Backspace or the
 * button in the row menu.
 */
export default function EditableCell({
  value,
  changed,
  editable,
  onCommit,
}: EditableCellProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  function start() {
    if (!editable) return;
    setDraft(value ?? "");
    setEditing(true);
  }

  function commit(next: Cell) {
    setEditing(false);
    if (next !== value) onCommit(next);
  }

  if (editing) {
    return (
      <input
        ref={inputRef}
        className="db-cell-input"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => commit(draft)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit(draft);
          } else if (e.key === "Escape") {
            e.preventDefault();
            setEditing(false);
          } else if ((e.ctrlKey || e.metaKey) && e.key === "Backspace") {
            e.preventDefault();
            commit(null);
          }
        }}
        aria-label="Cell value"
      />
    );
  }

  const classes = [
    "db-cell",
    editable && "db-cell-editable",
    changed && "db-cell-changed",
  ]
    .filter(Boolean)
    .join(" ");

  const display =
    value === null ? (
      <span className="u-muted">NULL</span>
    ) : value.length > CELL_MAX ? (
      <span title={value}>{value.slice(0, CELL_MAX)}…</span>
    ) : (
      value
    );

  if (!editable) return <span className={classes}>{display}</span>;

  return (
    <button
      type="button"
      className={classes}
      // Double-click to edit, matching the drill-in gesture DataTable already
      // uses; a single click would fight with selecting text in a cell.
      onDoubleClick={start}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === "F2") {
          e.preventDefault();
          start();
        }
      }}
      title="Double-click to edit"
    >
      {display}
    </button>
  );
}
