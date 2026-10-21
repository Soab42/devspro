import { useEffect, useMemo, useState } from "react";
import { Button, Callout, Card, EmptyState, Mono, Stack, Toolbar } from "../../../ui";
import { Eraser, RefreshCw, Search, X } from "../../../ui/icons";
import { cleanCaches, cleanSystem } from "../api";
import type { CleanOutcome } from "../api";
import { formatBytes, suggestedIds } from "../cleaner";
import FindingsTable from "./FindingsTable";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { useTabSlot } from "../tabState";
import { useCachesScan } from "./caches/useCachesScan";

/** What the backend offers as the trash entry on this OS. */
const TRASH = typeof navigator !== "undefined" && /Win/.test(navigator.platform) ? "the Recycle Bin" : "Trash";

/** User-level caches and package stores, plus the system section the backend adds. */
export default function CachesTab() {
  const { caches, system, error, systemError, scannedAt, scanned, scanning, progress, scan, cancel } = useCachesScan();
  const [selected, setSelected] = useTabSlot<Set<string>>("caches", "selected", new Set());
  const [reviewing, setReviewing] = useState(false);

  const all = useMemo(() => [...caches, ...system], [caches, system]);
  const chosen = all.filter((f) => selected.has(f.id) && !f.blocked_by);
  const chosenBytes = chosen.reduce((sum, f) => sum + f.bytes, 0);

  // A rescan can drop findings; the selection must not keep ids that are gone.
  useEffect(() => {
    const ids = new Set(all.map((f) => f.id));
    setSelected((prev) => (prev.size === 0 || [...prev].every((id) => ids.has(id)) ? prev : new Set([...prev].filter((id) => ids.has(id)))));
  }, [all, setSelected]);

  /** Each finding goes to the clean command of the tab that found it. */
  async function clean(ids: string[]): Promise<CleanOutcome[]> {
    const tabOf = new Map(all.map((f) => [f.id, f.tab]));
    const systemIds = ids.filter((id) => tabOf.get(id) === "system");
    const cacheIds = ids.filter((id) => tabOf.get(id) !== "system");
    const outcomes: CleanOutcome[] = [];
    try {
      if (cacheIds.length > 0) outcomes.push(...(await cleanCaches(cacheIds)));
      if (systemIds.length > 0) outcomes.push(...(await cleanSystem(systemIds)));
    } finally {
      setSelected(new Set());
      void scan();
    }
    return outcomes;
  }

  return (
    <Stack gap={4}>
      <Toolbar
        end={
          <>
            <Button disabled={scanning || all.length === 0} onClick={() => setSelected(new Set(suggestedIds(all)))}>
              Select suggested
            </Button>
            <Button variant="danger" icon={Eraser} disabled={scanning || chosen.length === 0} onClick={() => setReviewing(true)}>
              Clean selected{chosen.length > 0 ? ` (${formatBytes(chosenBytes)})` : ""}
            </Button>
          </>
        }
      >
        <Button variant="primary" icon={scanned ? RefreshCw : Search} loading={scanning} disabled={scanning} onClick={() => void scan()}>
          {scanned ? "Rescan" : "Scan"}
        </Button>
        {scanning && (
          <Button icon={X} onClick={cancel}>
            Cancel
          </Button>
        )}
      </Toolbar>

      {!scanning && <ScanStamp at={scannedAt} />}

      {scanning && (
        <Stack gap={1}>
          <span className="u-muted">
            {progress
              ? `Scanning ${progress.entries.toLocaleString()} entries, ${formatBytes(progress.bytes)} so far`
              : "Starting the scan"}
          </span>
          {progress && <Mono>{progress.current}</Mono>}
        </Stack>
      )}

      {error && <Callout tone="danger">{error}</Callout>}

      {!scanned && !scanning && (
        <EmptyState
          icon={Eraser}
          size="md"
          title="Find caches"
          description={`Scan for package-manager caches, browser caches and ${TRASH}. Nothing is removed until you review it.`}
        />
      )}

      {scanned && !error && (
        <Card title="Caches" padding="md">
          <FindingsTable
            findings={caches}
            selected={selected}
            onSelectionChange={setSelected}
            loading={scanning && caches.length === 0}
            empty={{ title: "No caches found", description: "Nothing here is taking noticeable space." }}
          />
        </Card>
      )}

      {scanned && (
        <Card title="System" padding="md">
          {systemError ? (
            <Callout tone="warning">System scan failed: {systemError}</Callout>
          ) : (
            <FindingsTable
              findings={system}
              selected={selected}
              onSelectionChange={setSelected}
              empty={{ title: "Nothing to clean", description: "No system-level space to reclaim." }}
            />
          )}
        </Card>
      )}

      {reviewing && (
        <ReviewCleanModal
          items={chosen}
          onConfirm={(ids) => clean(ids)}
          onClose={() => setReviewing(false)}
        />
      )}
    </Stack>
  );
}
