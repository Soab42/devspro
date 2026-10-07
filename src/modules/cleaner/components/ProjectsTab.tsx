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
import { groupProjects, idleLabel, suggestedPaths, toFinding, type ProjectGroup } from "./projects/projectsModel";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function ProjectsTab() {
  const [rows, setRows] = useState<ProjectJunk[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const scanId = useRef<string | null>(null);

  const scan = useCallback(async () => {
    const id = newScanId();
    scanId.current = id;
    setScanning(true);
    setError(null);
    setProgress(null);
    try {
      const found = await scanProjects(id);
      if (scanId.current !== id) return;
      setRows(found);
      setSelected(new Set());
    } catch (e) {
      if (scanId.current === id) setError(message(e));
    } finally {
      if (scanId.current === id) setScanning(false);
    }
  }, []);

  useEffect(() => {
    void scan();
    const unlisten = onScanProgress((p) => {
      if (p.scan_id === scanId.current) setProgress(p);
    });
    return () => {
      scanId.current = null;
      void unlisten.then((fn) => fn());
    };
  }, [scan]);

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
    if (scanId.current) void cancelScan(scanId.current);
  }

  const groups = useMemo(() => groupProjects(rows ?? []), [rows]);
  const chosen = useMemo(() => (rows ?? []).filter((r) => selected.has(r.artifact_path)), [rows, selected]);
  const chosenBytes = chosen.reduce((sum, r) => sum + r.bytes, 0);

  function toggle(path: string, on: boolean) {
    const next = new Set(selected);
    if (on) next.add(path);
    else next.delete(path);
    setSelected(next);
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
