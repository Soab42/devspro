import { useMemo, useState } from "react";
import { SearchInput, SideList, Stack } from "../../../ui";
import { formatBytes } from "../../dashboard/utils";
import type { TableInfo } from "../api";

export interface ObjectSidebarProps {
  objects: TableInfo[];
  loading: boolean;
  selected: string | null;
  /** Drops this schema's prefix from displayed names; keys stay qualified. */
  stripSchema?: string;
  onSelect: (name: string) => void;
}

/**
 * The tables and views of one database, always on screen.
 *
 * Tables and views are separate groups rather than one list with a badge:
 * which of the two an object is changes what you can do with it (a view can't
 * be edited), so it is worth knowing before you click rather than after.
 */
export default function ObjectSidebar({
  objects,
  loading,
  selected,
  stripSchema,
  onSelect,
}: ObjectSidebarProps) {
  const [query, setQuery] = useState("");

  const { tables, views } = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matching = q
      ? objects.filter((o) => o.name.toLowerCase().includes(q))
      : objects;
    return {
      tables: matching.filter((o) => o.kind !== "view"),
      views: matching.filter((o) => o.kind === "view"),
    };
  }, [objects, query]);

  function group(items: TableInfo[]) {
    return items.map((o) => ({
      key: o.name,
      label:
        stripSchema && o.name.startsWith(`${stripSchema}.`)
          ? o.name.slice(stripSchema.length + 1)
          : o.name,
      active: o.name === selected,
      onSelect: () => onSelect(o.name),
      // Rows and size are what you scan a table list for — which is the big
      // one, and which is empty.
      meta:
        o.kind === "view"
          ? undefined
          : `${o.rows_estimate.toLocaleString()} · ${formatBytes(o.size_bytes)}`,
    }));
  }

  return (
    <Stack gap={3} className="db-sidebar">
      {/* Outside the scrolling area: a filter box that scrolls away is no use
          in the database with 238 tables, which is the one you need it in. */}
      <SearchInput
        value={query}
        onChange={setQuery}
        placeholder="Filter tables…"
        aria-label="Filter tables and views"
        /* This is a column: left to grow, the box takes the leftover height
           and the input floats in the middle of it. */
        grow={false}
      />
      <div className="db-sidebar-list">
        {!loading && objects.length === 0 && (
          <p className="u-muted db-sidebar-empty">No tables in this database.</p>
        )}
        {!loading &&
          objects.length > 0 &&
          tables.length === 0 &&
          views.length === 0 && (
            <p className="u-muted db-sidebar-empty">Nothing matches “{query}”.</p>
          )}
        {tables.length > 0 && (
          <SideList title={`Tables (${tables.length})`} items={group(tables)} />
        )}
        {views.length > 0 && (
          <SideList title={`Views (${views.length})`} items={group(views)} />
        )}
      </div>
    </Stack>
  );
}
