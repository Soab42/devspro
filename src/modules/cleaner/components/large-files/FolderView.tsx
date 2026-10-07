import { useEffect, useState } from "react";
import { Button, Callout, Column, DataTable, IconButton, Mono, Select, Spinner, Stack } from "../../../../ui";
import { ChevronRight, Folder, File, FolderOpen } from "../../../../ui/icons";
import { FolderEntry, folderSizes } from "../../api";
import { formatBytes, newScanId } from "../../cleaner";
import { crumbsFor } from "./model";

interface Props {
  roots: string[];
  onReveal: (path: string) => void;
}

/** The biggest folders, one level at a time: open a folder to see what fills it. */
export default function FolderView({ roots, onReveal }: Props) {
  const [root, setRoot] = useState(roots[0] ?? "");
  const [path, setPath] = useState(roots[0] ?? "");
  const [rows, setRows] = useState<FolderEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!path) return;
    let live = true;
    setLoading(true);
    setError(null);
    folderSizes(newScanId(), path)
      .then((r) => live && setRows(r))
      .catch((e) => live && (setRows([]), setError(e instanceof Error ? e.message : String(e))))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [path]);

  if (roots.length === 0) return <Callout tone="info">No scan roots are set. Add one in the Cleaner settings.</Callout>;

  const crumbs = crumbsFor(root, path);
  const columns: Column<FolderEntry>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      render: (r) =>
        r.is_dir ? (
          <Button variant="link" size="sm" icon={Folder} onClick={() => setPath(r.path)}>
            {r.name}
          </Button>
        ) : (
          <Stack direction="row" gap={2} align="center">
            <File size={14} aria-hidden />
            <span>{r.name}</span>
          </Stack>
        ),
      sortValue: (r) => r.name.toLowerCase(),
    },
    { key: "size", header: "Size", align: "end", priority: 1, render: (r) => formatBytes(r.bytes), sortValue: (r) => r.bytes },
    { key: "entries", header: "Items", align: "end", priority: 3, render: (r) => (r.is_dir ? r.entries.toLocaleString() : "-"), sortValue: (r) => r.entries },
    {
      key: "actions",
      header: "",
      headerLabel: "Actions",
      align: "end",
      priority: 1,
      render: (r) => <IconButton icon={FolderOpen} label={`Reveal ${r.name} in Files`} size="sm" onClick={() => onReveal(r.path)} />,
    },
  ];

  return (
    <Stack gap={4}>
      <Stack direction="row" gap={2} align="center" wrap>
        {roots.length > 1 && (
          <Select
            size="sm"
            aria-label="Scan root"
            value={root}
            options={roots.map((r) => ({ value: r, label: r }))}
            onChange={(r) => {
              setRoot(r);
              setPath(r);
            }}
          />
        )}
        <nav aria-label="Folder path">
          <Stack direction="row" gap={1} align="center" wrap>
            {crumbs.map((c, i) => (
              <Stack key={c.path} direction="row" gap={1} align="center">
                {i > 0 && <ChevronRight size={14} aria-hidden />}
                {i === crumbs.length - 1 ? (
                  <Mono>{c.name}</Mono>
                ) : (
                  <Button variant="link" size="sm" onClick={() => setPath(c.path)}>
                    {c.name}
                  </Button>
                )}
              </Stack>
            ))}
          </Stack>
        </nav>
        {loading && <Spinner />}
      </Stack>
      {error && <Callout tone="danger">{error}</Callout>}
      <DataTable
        caption="Biggest folders"
        columns={columns}
        rows={rows}
        rowKey={(r) => r.path}
        defaultSort={{ key: "size", dir: "desc" }}
        loading={loading && rows.length === 0}
        empty={{ title: "Nothing here", description: "This folder is empty." }}
        onRowDoubleClick={(r) => r.is_dir && setPath(r.path)}
      />
    </Stack>
  );
}
