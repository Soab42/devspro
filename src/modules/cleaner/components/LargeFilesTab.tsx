import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useConfirm } from "../../../app/ConfirmContext";
import { Badge, Button, Callout, Checkbox, Column, DataTable, EmptyState, IconButton, Mono, SegmentedControl, Select, Spinner, Stack, Toolbar } from "../../../ui";
import { Eraser, FolderOpen, RefreshCw, Trash2, X } from "../../../ui/icons";
import { CleanerSettings, LargeFile, ScanProgress, cancelScan, getSettings, onScanProgress, removeFiles, scanLargeFiles } from "../api";
import { formatBytes, newScanId } from "../cleaner";
import FolderView from "./large-files/FolderView";
import { AGE_OPTIONS, KIND_LABEL, KIND_OPTIONS, SIZE_OPTIONS, filterFiles, formatDate, nearestSize, parentOf, toFinding } from "./large-files/model";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { hasSlot, setSlot, useTabRestore, useTabSlot } from "../tabState";

const TAB = "large-files";

type View = "files" | "folders";

const VIEWS: { value: View; label: string }[] = [
  { value: "files", label: "Large files" },
  { value: "folders", label: "Biggest folders" },
];

export default function LargeFilesTab() {
  const navigate = useNavigate();
  const confirmAction = useConfirm();
  const ready = useTabRestore(TAB);
  const [settings, setSettings] = useState<CleanerSettings | null>(null);
  const [view, setView] = useTabSlot<View>(TAB, "view", "files");
  const [minSize, setMinSize] = useTabSlot(TAB, "minSize", SIZE_OPTIONS[1].value);
  const [kind, setKind] = useTabSlot(TAB, "kind", "all");
  const [age, setAge] = useTabSlot(TAB, "age", "0");
  const [files, setFiles] = useTabSlot<LargeFile[] | null>(TAB, "files", null);
  const [scanned] = useTabSlot<{ min: number; age: number } | null>(TAB, "scannedFilters", null);
  const [scannedAt] = useTabSlot<number | null>(TAB, "scannedAt", null);
  const [selected, setSelected] = useTabSlot<Set<string>>(TAB, "selected", new Set());
  const [scanId] = useTabSlot<string | null>(TAB, "scanId", null);
  const [progress] = useTabSlot<ScanProgress | null>(TAB, "progress", null);
  const [error, setError] = useTabSlot<string | null>(TAB, "error", null);
  const [review, setReview] = useState<{ items: LargeFile[]; permanent: boolean } | null>(null);
  const [trashed, setTrashed] = useState(false);

  useEffect(() => {
    if (!ready) return;
    let alive = true;
    getSettings()
      .then((s) => {
        if (!alive) return;
        setSettings(s);
        // The setting is only the first guess; a filter already chosen or restored wins.
        if (!hasSlot(TAB, "minSize")) setMinSize(nearestSize(s.large_file_min_bytes));
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [ready, setMinSize, setError]);

  // The scan writes to the tab store, so it finishes into it even if this tab is left.
  const scan = useCallback(async () => {
    const id = newScanId();
    const min = Number(minSize);
    const days = Number(age);
    setSlot(TAB, "scanId", id, null as string | null);
    setSlot(TAB, "scanning", true, false);
    setSlot(TAB, "progress", null as ScanProgress | null, null);
    setSlot(TAB, "error", null as string | null, null);
    const off = await onScanProgress((p) => {
      if (p.scan_id === id) setSlot(TAB, "progress", p as ScanProgress | null, null);
    }).catch(() => () => {});
    try {
      const found = await scanLargeFiles(id, min, days > 0 ? days : null);
      setSlot(TAB, "files", found as LargeFile[] | null, null);
      setSlot(TAB, "scannedFilters", { min, age: days } as { min: number; age: number } | null, null);
      setSlot(TAB, "scannedAt", Date.now() as number | null, null);
      setSlot(TAB, "selected", new Set<string>(), new Set<string>());
    } catch (e) {
      setSlot(TAB, "error", (e instanceof Error ? e.message : String(e)) as string | null, null);
    } finally {
      off();
      setSlot(TAB, "scanId", null as string | null, null);
      setSlot(TAB, "scanning", false, false);
    }
  }, [minSize, age]);

  const visible = useMemo(
    () => filterFiles(files ?? [], Number(minSize), kind, Number(age), Math.floor(Date.now() / 1000)),
    [files, minSize, kind, age],
  );
  const chosen = visible.filter((f) => selected.has(f.path));
  const chosenBytes = chosen.reduce((sum, f) => sum + f.bytes, 0);
  const stale = scanned !== null && (Number(minSize) < scanned.min || (scanned.age > 0 && (Number(age) === 0 || Number(age) < scanned.age)));
  const scanning = scanId !== null;

  const reveal = (path: string) => navigate(`/files?path=${encodeURIComponent(parentOf(path))}`);
  const toggle = (path: string, on: boolean) =>
    setSelected((cur) => {
      const next = new Set(cur);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });
  const allOn = visible.length > 0 && chosen.length === visible.length;

  async function deletePermanently() {
    const ok = await confirmAction(
      `Permanently delete ${chosen.length} file${chosen.length === 1 ? "" : "s"} (${formatBytes(chosenBytes)})? They will not go to the Trash and cannot be restored.`,
      { title: "Delete permanently", confirmLabel: "Continue", danger: true },
    );
    if (ok) setReview({ items: chosen, permanent: true });
  }

  const columns: Column<LargeFile>[] = [
    {
      key: "select",
      header: <Checkbox label="Select all" hideLabel checked={allOn} indeterminate={chosen.length > 0 && !allOn} onChange={(on) => setSelected(on ? new Set(visible.map((f) => f.path)) : new Set())} />,
      headerLabel: "Select",
      width: "2.5rem",
      priority: 1,
      render: (f) => <Checkbox label={`Select ${f.path}`} hideLabel checked={selected.has(f.path)} onChange={(on) => toggle(f.path, on)} />,
    },
    { key: "path", header: "Path", priority: 1, mono: true, render: (f) => f.path, sortValue: (f) => f.path },
    { key: "size", header: "Size", align: "end", priority: 1, render: (f) => formatBytes(f.bytes), sortValue: (f) => f.bytes },
    { key: "kind", header: "Kind", priority: 2, render: (f) => <Badge>{KIND_LABEL[f.kind]}</Badge>, sortValue: (f) => f.kind },
    { key: "modified", header: "Modified", priority: 2, render: (f) => formatDate(f.modified), sortValue: (f) => f.modified },
    {
      key: "actions",
      header: "",
      headerLabel: "Actions",
      align: "end",
      priority: 1,
      render: (f) => <IconButton icon={FolderOpen} label={`Reveal ${f.path} in Files`} size="sm" onClick={() => reveal(f.path)} />,
    },
  ];

  const roots = settings?.scan_roots ?? [];

  return (
    <Stack gap={4}>
      <Toolbar
        end={
          scanning ? (
            <Button icon={X} onClick={() => scanId && void cancelScan(scanId)}>
              Cancel
            </Button>
          ) : view === "files" ? (
            <Button variant="primary" icon={RefreshCw} onClick={() => void scan()}>
              {files ? "Rescan" : "Scan"}
            </Button>
          ) : undefined
        }
      >
        <SegmentedControl ariaLabel="View" options={VIEWS} value={view} onChange={setView} />
        {view === "files" && (
          <>
            <SegmentedControl ariaLabel="Minimum size" options={SIZE_OPTIONS} value={minSize} onChange={setMinSize} />
            <Select size="sm" aria-label="Kind" value={kind} options={KIND_OPTIONS} onChange={setKind} />
            <Select size="sm" aria-label="Age" value={age} options={AGE_OPTIONS} onChange={setAge} />
          </>
        )}
      </Toolbar>

      {error && <Callout tone="danger">{error}</Callout>}
      {trashed && (
        <Callout tone="info" onDismiss={() => setTrashed(false)}>
          The files are in the Trash. The space is freed when the Trash is emptied (Caches tab).
        </Callout>
      )}

      {view === "folders" ? (
        settings ? <FolderView roots={roots} onReveal={reveal} /> : <Spinner />
      ) : scanning ? (
        <Stack direction="row" gap={2} align="center">
          <Spinner />
          <span>
            Scanning, {(progress?.entries ?? 0).toLocaleString()} entries
            {progress ? <> · <Mono>{progress.current}</Mono></> : null}
          </span>
        </Stack>
      ) : files === null ? (
        <EmptyState
          icon={Eraser}
          size="md"
          title="Find large files"
          description="Scans your scan roots for big files. Caches, package stores and project build folders are covered by other tabs."
        />
      ) : (
        <>
          <ScanStamp at={scannedAt} />
          {stale && <Callout tone="info">The filters are looser than this scan. Rescan to include more files.</Callout>}
          <Stack direction="row" gap={2} align="center" wrap>
            <Button icon={Trash2} disabled={chosen.length === 0} onClick={() => setReview({ items: chosen, permanent: false })}>
              Move to Trash
            </Button>
            <Button variant="danger" icon={Trash2} disabled={chosen.length === 0} onClick={() => void deletePermanently()}>
              Delete permanently
            </Button>
            <span>
              {chosen.length > 0 ? `${chosen.length} selected, ${formatBytes(chosenBytes)}` : `${visible.length} files`}
            </span>
          </Stack>
          <DataTable
            caption="Large files"
            columns={columns}
            rows={visible}
            rowKey={(f) => f.path}
            defaultSort={{ key: "size", dir: "desc" }}
            empty={{ title: "No large files", description: "Nothing matches these filters." }}
          />
        </>
      )}

      {review && (
        <ReviewCleanModal
          items={review.items.map((f) => toFinding(f, review.permanent))}
          onClose={() => setReview(null)}
          onConfirm={async (ids) => {
            const outcomes = await removeFiles(ids, review.permanent);
            if (!review.permanent && outcomes.some((o) => o.ok)) setTrashed(true);
            const gone = new Set(outcomes.filter((o) => o.ok).map((o) => o.id));
            setFiles((cur) => cur && cur.filter((f) => !gone.has(f.path)));
            setSelected((cur) => new Set([...cur].filter((p) => !gone.has(p))));
            return outcomes;
          }}
        />
      )}
    </Stack>
  );
}
