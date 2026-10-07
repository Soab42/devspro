import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Callout, Stack, Toolbar } from "../../../ui";
import { Eraser, RefreshCw } from "../../../ui/icons";
import { cleanDocker, scanDocker } from "../api";
import type { CleanOutcome, DockerScan } from "../api";
import { formatBytes, newScanId, suggestedIds } from "../cleaner";
import FindingsTable from "./FindingsTable";
import ReviewCleanModal from "./ReviewCleanModal";
import UnavailableCallout from "./docker/UnavailableCallout";

/** Docker's leftovers, one row each, grouped by kind; removes what is ticked. */
export default function DockerTab() {
  const [result, setResult] = useState<DockerScan | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [reviewing, setReviewing] = useState(false);
  const cleaned = useRef(false);

  const scan = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setResult(await scanDocker(newScanId()));
      setSelected(new Set());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void scan();
  }, [scan]);

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
