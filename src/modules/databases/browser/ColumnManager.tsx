import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Button, Checkbox, Icon, IconButton } from "../../../ui";
import { ChevronLeft, ChevronRight, Columns2, Folder } from "../../../ui/icons";
import {
  clearedColumns,
  moveColumn,
  orderedColumns,
  toggleHidden,
} from "./grid";
import "./browser.css";

export interface ColumnManagerProps {
  /** Every column of the table, in the table's own order. */
  columns: string[];
  hidden: Set<string>;
  onChange: (hidden: Set<string>) => void;
  /** The chosen order — partial, and a preference rather than a schema. */
  order: string[];
  onReorder: (order: string[]) => void;
  /** What a column is called, where the key is not the name — a relation. */
  labelOf?: (key: string) => string;
  /** Marks a key as a related table, drawn with a folder beside its name. */
  isRelation?: (key: string) => boolean;
}

/**
 * Which columns the grid draws.
 *
 * A popover rather than a modal: choosing columns is something you do *while*
 * looking at the grid, comparing what is on screen against the list, and a
 * dialog over the top of it would hide the thing being adjusted.
 */
export default function ColumnManager({
  columns,
  hidden,
  onChange,
  order,
  onReorder,
  labelOf,
  isRelation,
}: ColumnManagerProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; right: number } | null>(
    null,
  );
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // Portalled and viewport-anchored for the same reason Select's listbox is:
  // the grid is a scroll container, and anchored inside it the popover would
  // be clipped at the table's edge.
  useEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setPosition({
      top: rect.bottom + 4,
      right: Math.max(8, window.innerWidth - rect.right),
    });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onDown(e: MouseEvent) {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const shownCount = columns.length - hidden.size;
  // Listed in the order the grid draws them, so the arrows move a column the
  // way it visibly moves rather than the way the table happens to declare it.
  const listed = orderedColumns(columns, order);
  const move = (column: string, delta: -1 | 1) =>
    moveColumn(columns, order, column, delta);

  return (
    <>
      <IconButton
        ref={triggerRef}
        icon={Columns2}
        label={`Columns (${shownCount} of ${columns.length} shown)`}
        size="sm"
        variant="secondary"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      />
      {open &&
        createPortal(
          <div
            ref={panelRef}
            className="db-column-manager"
            role="dialog"
            aria-label="Column manager"
            style={{
              top: position?.top ?? -9999,
              right: position?.right ?? -9999,
              visibility: position ? "visible" : "hidden",
            }}
          >
            <div className="db-column-manager-head">
              <span className="db-column-manager-title">Columns</span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onChange(new Set())}
              >
                Select all
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onChange(clearedColumns(columns))}
              >
                Clear
              </Button>
            </div>
            <div className="db-column-manager-list">
              {listed.map((column, index) => (
                <div className="db-column-manager-row" key={column}>
                  <Checkbox
                    checked={!hidden.has(column)}
                    label={labelOf?.(column) ?? column}
                    adornment={
                      isRelation?.(column) ? (
                        <Icon icon={Folder} size="xs" />
                      ) : undefined
                    }
                    onChange={() => onChange(toggleHidden(hidden, column))}
                  />
                  {/* The other way to reorder, for anyone who would rather not
                      drag a header — and the only way to move a column that is
                      currently hidden. */}
                  <span className="db-column-manager-move">
                    <IconButton
                      icon={ChevronLeft}
                      label={`Move ${labelOf?.(column) ?? column} left`}
                      size="sm"
                      variant="ghost"
                      disabled={index === 0}
                      onClick={() => onReorder(move(column, -1))}
                    />
                    <IconButton
                      icon={ChevronRight}
                      label={`Move ${labelOf?.(column) ?? column} right`}
                      size="sm"
                      variant="ghost"
                      disabled={index === listed.length - 1}
                      onClick={() => onReorder(move(column, 1))}
                    />
                  </span>
                </div>
              ))}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
