import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Checkbox, DataTable, EmptyState, Mono, Spinner, Stack, Toolbar } from "../../../ui";
import type { Column } from "../../../ui";
import { FolderPlus, RefreshCw, Eraser } from "../../../ui/icons";
import {
  cancelScan,
  cleanProjects,
  getSettings,
  onScanProgress,
  scanProjects,
  setSettings,
  type ProjectJunk,
  type ScanProgress,
} from "../api";
import { formatBytes, newScanId } from "../cleaner";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { getSlot, setSlot, useTabRestore, useTabSlot } from "../tabState";
import { groupProjects, idleLabel, suggestedPaths, toFinding, type ProjectGroup } from "./projects/projectsModel";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const TAB = "projects";

export default function ProjectsTab() {
  const ready = useTabRestore(TAB);
  const [rows] = useTabSlot<ProjectJunk[] | null>(TAB, "rows", null);
  const [selected, setSelected] = useTabSlot<Set<string>>(TAB, "selected", new Set());
  const [scanning] = useTabSlot(TAB, "scanning", false);
  const [progress] = useTabSlot<ScanProgress | null>(TAB, "progress", null);
  const [error, setError] = useTabSlot<string | null>(TAB, "error", null);
  const [scannedAt] = useTabSlot<number | null>(TAB, "scannedAt", null);
  const [reviewing, setReviewing] = useState(false);
  const autoScanned = useRef(false);

  // The scan writes to the tab store, so it finishes into it even if this tab is left.
  const scan = useCallback(async () => {
    if (getSlot(TAB, "scanning", false)) return;
    const id = newScanId();
    setSlot(TAB, "scanId", id, null as string | null);
    setSlot(TAB, "scanning", true, false);
    setSlot(TAB, "error", null as string | null, null);
    setSlot(TAB, "progress", null as ScanProgress | null, null);
    const off = await onScanProgress((p) => {
      if (p.scan_id === id) setSlot(TAB, "progress", p as ScanProgress | null, null);
    }).catch(() => () => {});
    try {
      const found = await scanProjects(id);
      setSlot(TAB, "rows", found as ProjectJunk[] | null, null);
      setSlot(TAB, "scannedAt", Date.now() as number | null, null);
      setSlot(TAB, "selected", new Set<string>(), new Set<string>());
    } catch (e) {
      setSlot(TAB, "error", message(e) as string | null, null);
    } finally {
      off();
      setSlot(TAB, "scanId", null as string | null, null);
      setSlot(TAB, "scanning", false, false);
    }
  }, []);

  // Only a tab with nothing to show scans by itself; a restored one waits for Rescan.
  useEffect(() => {
    if (!ready || autoScanned.current || rows !== null) return;
    autoScanned.current = true;
    void scan();
  }, [ready, rows, scan]);

  async function addFolder() {
    try {
      const picked = await open({ directory: true, multiple: false, title: "Select a project folder" });
      if (!picked || Array.isArray(picked)) return;
      const settings = await getSettings();
      if (!settings.project_roots.includes(picked)) {
        await setSettings({ ...settings, project_roots: [...settings.project_roots, picked] });
      }
      await scan();
    } catch (e) {
      setError(message(e));
    }
  }

  function cancel() {
    const id = getSlot<string | null>(TAB, "scanId", null);
    if (id) void cancelScan(id);
  }

  const groups = useMemo(() => groupProjects(rows ?? []), [rows]);
  const chosen = useMemo(() => (rows ?? []).filter((r) => selected.has(r.artifact_path)), [rows, selected]);
  const chosenBytes = chosen.reduce((sum, r) => sum + r.bytes, 0);

  function toggle(path: string, on: boolean) {
    setSelected((cur) => {
      const next = new Set(cur);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });
  }

  const columns: Column<ProjectGroup>[] = [
    {
      key: "project",
      header: "Project",
      render: (g) => (
        <Stack gap={1}>
          <strong>{g.name}</strong>
          <Mono>{g.path}</Mono>
        </Stack>
      ),
    },
    { key: "total", header: "Total", align: "end", mono: true, render: (g) => formatBytes(g.total) },
    { key: "idle", header: "Idle", render: (g) => idleLabel(g.idleDays) },
    {
      key: "artifacts",
      header: "Artifacts",
      wrap: true,
      render: (g) => (
        <Stack gap={1}>
          {g.artifacts.map((a) => (
            <Checkbox
              key={a.artifact_path}
              label={`${a.artifact_kind} ${formatBytes(a.bytes)}`}
              checked={selected.has(a.artifact_path)}
              onChange={(on) => toggle(a.artifact_path, on)}
            />
          ))}
        </Stack>
      ),
    },
  ];

  const addButton = (
    <Button icon={FolderPlus} onClick={addFolder}>
      Add project folder
    </Button>
  );

  if (rows === null && scanning) return <ScanStatus progress={progress} onCancel={cancel} />;

  return (
    <Stack gap={4}>
      {error && <Callout tone="danger">{error}</Callout>}
      {scanning && <ScanStatus progress={progress} onCancel={cancel} />}
      {rows === null && !scanning && error && <Toolbar>{addButton}</Toolbar>}
      {rows !== null && rows.length === 0 && !scanning && (
        <EmptyState
          icon={Eraser}
          size="md"
          title="No project junk found"
          description="Add the folders your projects live in. node_modules, target, vendor and similar folders are listed with how long each project has been idle."
          action={addButton}
        />
      )}
      {rows !== null && rows.length > 0 && (
        <>
          <Toolbar
            end={
              <Button variant="danger" icon={Eraser} disabled={chosen.length === 0} onClick={() => setReviewing(true)}>
                Clean {chosen.length > 0 ? formatBytes(chosenBytes) : "selected"}
              </Button>
            }
          >
            <Button onClick={() => setSelected(new Set(suggestedPaths(rows)))}>Select suggested</Button>
            {addButton}
            <Button icon={RefreshCw} disabled={scanning} onClick={() => void scan()}>
              Rescan
            </Button>
          </Toolbar>
          {!scanning && <ScanStamp at={scannedAt} />}
          <DataTable columns={columns} rows={groups} rowKey={(g) => g.path} caption="Project junk" />
        </>
      )}
      {reviewing && (
        <ReviewCleanModal
          items={chosen.map(toFinding)}
          onConfirm={(ids) => cleanProjects(ids)}
          onClose={() => {
            setReviewing(false);
            void scan();
          }}
        />
      )}
    </Stack>
  );
}

function ScanStatus({ progress, onCancel }: { progress: ScanProgress | null; onCancel: () => void }) {
  return (
    <Stack gap={2} direction="row" align="center">
      <Spinner label="Scanning projects" />
      <span>
        Scanning{progress ? ` ${progress.entries} entries, ${formatBytes(progress.bytes)}` : "…"}
      </span>
      {progress && <Mono>{progress.current}</Mono>}
      <Button onClick={onCancel}>Cancel</Button>
    </Stack>
  );
}
