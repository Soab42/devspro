import { DragEvent, useState } from "react";
import { Icon } from "../../../ui";
import { Folder, X } from "../../../ui/icons";
import { entryIcon } from "../entryIcon";
import type { OpenFile } from "../openFiles";
import { baseName, parentPath } from "../utils";

/**
 * The tab row above the folder view: the folder view itself first (never
 * closed), then every open file, editor-style. Middle-click closes, a dot
 * marks unsaved edits, and two files of the same name say which folder each
 * is in. Tabs can be dragged into another order.
 */
export default function FileTabStrip({
  files,
  active,
  dirty,
  describe,
  folderLabel,
  onActivate,
  onClose,
  onReorder,
}: {
  files: OpenFile[];
  active: string | null;
  dirty: string[];
  /** The path as the tooltip shows it. */
  describe: (path: string) => string;
  /** The folder the Files tab is showing. */
  folderLabel: string;
  onActivate: (path: string | null) => void;
  onClose: (path: string) => void;
  onReorder: (path: string, before: string | null) => void;
}) {
  const [dragging, setDragging] = useState<string | null>(null);
  const [dropBefore, setDropBefore] = useState<string | null>(null);
  const counts = new Map<string, number>();
  for (const f of files) counts.set(f.name, (counts.get(f.name) ?? 0) + 1);

  function dropProps(path: string) {
    return {
      onDragOver: (e: DragEvent) => {
        if (!dragging || dragging === path) return;
        e.preventDefault();
        if (dropBefore !== path) setDropBefore(path);
      },
      onDrop: (e: DragEvent) => {
        e.preventDefault();
        if (dragging && dragging !== path) onReorder(dragging, path);
        setDragging(null);
        setDropBefore(null);
      },
    };
  }

  return (
    <div className="fm-tabs" role="tablist" aria-label="Open files">
      <button
        type="button"
        role="tab"
        aria-selected={active === null}
        className={active === null ? "fm-tab fm-tab-folder fm-tab-active" : "fm-tab fm-tab-folder"}
        title={folderLabel}
        onClick={() => onActivate(null)}
      >
        <Icon icon={Folder} size="xs" className="fm-col-icon-dir" />
        <span className="fm-tab-name">{folderLabel}</span>
      </button>
      {files.map((f) => {
        const isActive = f.path === active;
        const isDirty = dirty.includes(f.path);
        const where = (counts.get(f.name) ?? 0) > 1 ? baseName(parentPath(f.path)) : null;
        const cls = ["fm-tab", isActive && "fm-tab-active", isDirty && "fm-tab-dirty", dropBefore === f.path && "fm-tab-drop"].filter(Boolean).join(" ");
        return (
          <button
            key={f.path}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={cls}
            title={describe(f.path)}
            draggable
            onClick={() => onActivate(f.path)}
            onAuxClick={(e) => {
              if (e.button === 1) onClose(f.path);
            }}
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", f.path);
              setDragging(f.path);
            }}
            onDragEnd={() => {
              setDragging(null);
              setDropBefore(null);
            }}
            {...dropProps(f.path)}
          >
            <Icon icon={entryIcon({ name: f.name, is_dir: false, is_symlink: false })} size="xs" className="fm-col-icon" />
            <span className="fm-tab-name">{f.name}</span>
            {where && <span className="fm-tab-where">{where}</span>}
            <span
              role="button"
              tabIndex={-1}
              aria-label={isDirty ? `Close ${f.name} (unsaved changes)` : `Close ${f.name}`}
              className="fm-tab-close"
              onClick={(e) => {
                e.stopPropagation();
                onClose(f.path);
              }}
            >
              <Icon icon={X} size="xs" />
            </span>
          </button>
        );
      })}
      {/* The strip's empty end: dropping here moves a tab to the last place. */}
      <div
        className={dropBefore === "" ? "fm-tabs-end fm-tab-drop" : "fm-tabs-end"}
        onDragOver={(e) => {
          if (!dragging) return;
          e.preventDefault();
          if (dropBefore !== "") setDropBefore("");
        }}
        onDrop={(e) => {
          e.preventDefault();
          if (dragging) onReorder(dragging, null);
          setDragging(null);
          setDropBefore(null);
        }}
      />
    </div>
  );
}
