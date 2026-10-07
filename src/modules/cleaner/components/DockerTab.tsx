import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Callout, Stack, Toolbar } from "../../../ui";
import { Eraser, RefreshCw } from "../../../ui/icons";
import { cleanDocker, scanDocker } from "../api";
import type { CleanOutcome, DockerScan } from "../api";
import { formatBytes, newScanId, suggestedIds } from "../cleaner";
import FindingsTable from "./FindingsTable";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { getSlot, setSlot, useTabRestore, useTabSlot } from "../tabState";
import UnavailableCallout from "./docker/UnavailableCallout";

/** Docker's leftovers, one row each, grouped by kind; removes what is ticked. */
export default function DockerTab() {
  const ready = useTabRestore("docker");
  const [result] = useTabSlot<DockerScan | null>("docker", "result", null);
  const [loading] = useTabSlot("docker", "scanning", false);
  const [error] = useTabSlot<string | null>("docker", "error", null);
  const [scannedAt] = useTabSlot<number | null>("docker", "scannedAt", null);
  const [selected, setSelected] = useTabSlot<Set<string>>("docker", "selected", new Set());
  const [reviewing, setReviewing] = useState(false);
  const cleaned = useRef(false);
  const autoScanned = useRef(false);

  // The scan writes to the tab store, so it finishes into it even if this tab is left.
  const scan = useCallback(async () => {
    if (getSlot("docker", "scanning", false)) return;
    setSlot("docker", "scanning", true, false);
    setSlot<string | null>("docker", "error", null, null);
    try {
      const found = await scanDocker(newScanId());
      setSlot<DockerScan | null>("docker", "result", found, null);
      setSlot<number | null>("docker", "scannedAt", Date.now(), null);
      setSlot("docker", "selected", new Set<string>(), new Set<string>());
    } catch (e) {
      setSlot<string | null>("docker", "error", e instanceof Error ? e.message : String(e), null);
    }
    setSlot("docker", "scanning", false, false);
  }, []);

  // Only a tab with nothing to show scans by itself; a restored one waits for Rescan.
  useEffect(() => {
    if (!ready || autoScanned.current || result !== null) return;
    autoScanned.current = true;
    void scan();
  }, [ready, result, scan]);

  const findings = result?.findings ?? [];
  const chosen = findings.filter((f) => selected.has(f.id));
  const chosenBytes = chosen.reduce((sum, f) => sum + f.bytes, 0);

  async function onConfirm(ids: string[], confirmed: string[]): Promise<CleanOutcome[]> {
    cleaned.current = true;
    return cleanDocker(ids, confirmed);
  }

  function closeReview() {
    setReviewing(false);
    if (cleaned.current) {
      cleaned.current = false;
      void scan();
    }
  }

  return (
    <Stack gap={4}>
      <Toolbar>
        <Button icon={RefreshCw} onClick={scan} disabled={loading}>
          {result ? "Rescan" : "Scan"}
        </Button>
        <Button disabled={loading || !result?.available} onClick={() => setSelected(new Set(suggestedIds(findings)))}>
          Select suggested
        </Button>
        <Button
          variant="danger"
          icon={Eraser}
          disabled={chosen.length === 0}
          onClick={() => setReviewing(true)}
        >
          {chosen.length > 0 ? `Review and clean ${formatBytes(chosenBytes)}` : "Review and clean"}
        </Button>
      </Toolbar>

      {error && <Callout tone="danger">{error}</Callout>}
      {!loading && <ScanStamp at={scannedAt} />}

      {result && !result.available ? (
        <UnavailableCallout message={result.message} onRecheck={scan} />
      ) : (
        <FindingsTable
          findings={findings}
          selected={selected}
          onSelectionChange={setSelected}
          loading={loading && !result}
          empty={{ title: "Docker is tidy", description: "No leftover containers, images, networks, volumes or build cache." }}
        />
      )}

      {reviewing && <ReviewCleanModal items={chosen} onConfirm={onConfirm} onClose={closeReview} />}
    </Stack>
  );
}
