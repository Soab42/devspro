import { useCallback, useEffect, useMemo, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { Badge, BadgeTone, Button, Callout, Card, Meter, Section, Spinner, Stack, useResource } from "../../../ui";
import { Eraser, HardDrive, RefreshCw, Search, Square, Trash2 } from "../../../ui/icons";
import { fetchSystemSnapshot } from "../../dashboard/api";
import { percent } from "../../dashboard/utils";
import { cancelScan, clearHistory, listHistory, onScanProgress } from "../api";
import type { CleanOutcome, Finding, HistoryEntry, Risk, ScanProgress } from "../api";
import { formatBytes, newScanId, RISK_LABEL, RISKS, suggestedIds, totalsByRisk } from "../cleaner";
import CleanerSettings from "./CleanerSettings";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { getSlot, setSlot, useTabRestore, useTabSlot } from "../tabState";
import { readLastScan, timeAgo, writeLastScan } from "./overview/lastScan";
import type { LastScan } from "./overview/lastScan";
import { cleanFindings, errorText, runScan, SOURCES } from "./overview/scans";
import type { Source, SourceResult } from "./overview/scans";

const RISK_TONE: Record<Risk, BadgeTone> = { safe: "success", review: "warning", danger: "danger" };

const TAB = "overview";

type Results = Partial<Record<Source, SourceResult>>;

export default function OverviewTab() {
  const confirmAction = useConfirm();
  const snapshot = useResource(fetchSystemSnapshot);
  useTabRestore(TAB);
  const [results, setResults] = useTabSlot<Results>(TAB, "results", {});
  const [running, setRunning] = useTabSlot<Set<Source>>(TAB, "running", new Set());
  const [progress, setProgress] = useTabSlot<Partial<Record<Source, ScanProgress>>>(TAB, "progress", {});
  const [scannedAt] = useTabSlot<number | null>(TAB, "scannedAt", null);
  const [reviewing, setReviewing] = useState<Finding[] | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [lastScan, setLastScan] = useState<LastScan | null>(readLastScan);

  const refreshHistory = useCallback(async () => {
    try {
      setHistory(await listHistory(50));
      setHistoryError(null);
    } catch (e) {
      setHistoryError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void refreshHistory();
  }, [refreshHistory]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    onScanProgress((p) => {
      const ids = getSlot<Partial<Record<Source, string>>>(TAB, "scanIds", {});
      const source = SOURCES.find((s) => ids[s.id] === p.scan_id)?.id;
      if (source) setProgress((prev) => ({ ...prev, [source]: p }));
    }).then(
      (un) => (gone ? un() : (off = un)),
      () => {},
    );
    return () => {
      gone = true;
      off?.();
    };
  }, [setProgress]);

  const findings = useMemo(() => SOURCES.flatMap((s) => results[s.id]?.findings ?? []), [results]);
  const totals = useMemo(() => totalsByRisk(findings), [findings]);
  const suggested = useMemo(() => {
    const ids = new Set(suggestedIds(findings));
    return findings.filter((f) => ids.has(f.id));
  }, [findings]);
  const scanning = running.size > 0;
  const scanned = Object.keys(results).length > 0;

  function remember(next: Results) {
    const all = SOURCES.flatMap((s) => next[s.id]?.findings ?? []);
    const saved = { at: new Date().toISOString(), totals: totalsByRisk(all) };
    writeLastScan(saved);
    setLastScan(saved);
  }

  // Runs through the tab store, so a scan still going when this tab is left finishes into it.
  async function scanAll() {
    if (getSlot(TAB, "scanning", false)) return;
    const ids: Partial<Record<Source, string>> = {};
    for (const s of SOURCES) ids[s.id] = newScanId();
    setSlot(TAB, "scanIds", ids, {} as Partial<Record<Source, string>>);
    setSlot(TAB, "scanning", true, false);
    setResults({});
    setProgress({});
    setRunning(new Set(SOURCES.map((s) => s.id)));
    const next: Results = {};
    await Promise.all(
      SOURCES.map(async (s) => {
        const r = await runScan(s.id, ids[s.id] as string);
        next[s.id] = r;
        setResults((prev) => ({ ...prev, [s.id]: r }));
        setRunning((prev) => {
          const left = new Set(prev);
          left.delete(s.id);
          return left;
        });
      }),
    );
    setSlot(TAB, "scannedAt", Date.now() as number | null, null);
    setSlot(TAB, "scanning", false, false);
    if (SOURCES.some((s) => !next[s.id]?.error)) remember(next);
  }

  function cancel() {
    const ids = getSlot<Partial<Record<Source, string>>>(TAB, "scanIds", {});
    for (const id of running) {
      const scanId = ids[id];
      if (scanId) cancelScan(scanId).catch(() => {});
    }
  }

  async function runClean(ids: string[], confirmed: string[]): Promise<CleanOutcome[]> {
    const chosen = new Set(ids);
    const outcomes = await cleanFindings(
      findings.filter((f) => chosen.has(f.id)),
      confirmed,
    );
    const freed = new Set(outcomes.filter((o) => o.ok).map((o) => o.id));
    setResults((prev) => {
      const next: Results = {};
      for (const s of SOURCES) {
        const r = prev[s.id];
        if (r) next[s.id] = { ...r, findings: r.findings.filter((f) => !freed.has(f.id)) };
      }
      return next;
    });
    return outcomes;
  }

  function closeReview() {
    setReviewing(null);
    void refreshHistory();
    void snapshot.refresh();
  }

  async function clear() {
    const ok = await confirmAction("Clear the cleanup history? This does not restore anything.", {
      title: "Clear history",
      confirmLabel: "Clear",
      danger: true,
    });
    if (!ok) return;
    try {
      await clearHistory();
      setHistory([]);
    } catch (e) {
      setHistoryError(errorText(e));
    }
  }

  const bySource = SOURCES.map((s) => ({
    ...s,
    bytes: (results[s.id]?.findings ?? []).reduce((sum, f) => sum + (f.blocked_by ? 0 : f.bytes), 0),
  }));
  const suggestedBytes = suggested.reduce((sum, f) => sum + f.bytes, 0);
  const disks = snapshot.data?.disks ?? [];

  return (
    <Stack gap={5}>
      <Section title="Disks" icon={HardDrive} headingLevel={2} isEmpty={disks.length === 0} empty={{ icon: HardDrive, title: snapshot.error ?? "No mounted disks found" }}>
        <Stack gap={4}>
          {disks.map((d) => (
            <Meter
              key={d.mount_point}
              label={d.mount_point}
              percent={percent(d.used_bytes, d.total_bytes)}
              valueLabel={`${formatBytes(d.used_bytes)} / ${formatBytes(d.total_bytes)}`}
            />
          ))}
        </Stack>
      </Section>

      <Card
        title="Space you can free"
        icon={Eraser}
        description={lastScan && !scanned ? `Last scan ${timeAgo(lastScan.at)}: about ${formatBytes(lastScan.totals.safe + lastScan.totals.review + lastScan.totals.danger)} found.` : undefined}
        actions={
          <Stack direction="row" gap={2}>
            {scanning ? (
              <Button icon={Square} onClick={cancel}>
                Cancel
              </Button>
            ) : (
              <Button icon={Search} onClick={scanAll}>
                Scan everything
              </Button>
            )}
            <Button variant="primary" icon={Eraser} disabled={scanning || suggested.length === 0} onClick={() => setReviewing(suggested)}>
              Clean suggested
            </Button>
          </Stack>
        }
      >
        <Stack gap={4}>
          {scanning && (
            <Stack gap={1}>
              {SOURCES.filter((s) => running.has(s.id)).map((s) => {
                const p = progress[s.id];
                return (
                  <Stack key={s.id} direction="row" gap={2} align="center">
                    <Spinner />
                    <span>
                      Scanning {s.label.toLowerCase()}
                      {p ? `: ${p.entries.toLocaleString()} entries, ${formatBytes(p.bytes)}` : "…"}
                    </span>
                  </Stack>
                );
              })}
            </Stack>
          )}
          {!scanned && !scanning && <p>Scan everything to see what can be freed. Nothing is removed until you review it.</p>}
          {scanned && (
            <>
              <Stack direction="row" gap={3} wrap>
                {RISKS.map((r) => (
                  <Badge key={r} tone={RISK_TONE[r]} size="md">
                    {RISK_LABEL[r]}: {formatBytes(totals[r])}
                  </Badge>
                ))}
              </Stack>
              <Stack direction="row" gap={3} wrap>
                {bySource.map((s) => (
                  <Badge key={s.id} tone="neutral" size="md">
                    {s.label}: {results[s.id] ? formatBytes(s.bytes) : "…"}
                  </Badge>
                ))}
              </Stack>
              {!scanning && <ScanStamp at={scannedAt} />}
              {suggested.length > 0 && !scanning && (
                <p>
                  {suggested.length} suggested {suggested.length === 1 ? "item" : "items"}, {formatBytes(suggestedBytes)}.
                </p>
              )}
              {SOURCES.map((s) => {
                const r = results[s.id];
                if (r?.error) return <Callout key={s.id} tone="danger">{s.label} scan failed: {r.error}</Callout>;
                if (r?.note) return <Callout key={s.id} tone="info">{s.label}: {r.note}</Callout>;
                return null;
              })}
            </>
          )}
        </Stack>
      </Card>

      <Section
        title="History"
        icon={RefreshCw}
        actions={
          <Button size="sm" icon={Trash2} disabled={history.length === 0} onClick={clear}>
            Clear
          </Button>
        }
        isEmpty={history.length === 0 && !historyError}
        empty={{ title: "Nothing cleaned yet" }}
      >
        <Stack gap={2}>
          {historyError && <Callout tone="danger">{historyError}</Callout>}
          {history.map((h) => (
            <Stack key={h.id} direction="row" gap={3} align="center" wrap>
              <Badge tone={h.ok ? "success" : "danger"}>{h.ok ? formatBytes(h.bytes_freed) : "Failed"}</Badge>
              <span>{h.label}</span>
              <span>{timeAgo(h.at)}</span>
              {!h.ok && <span>{h.message}</span>}
            </Stack>
          ))}
        </Stack>
      </Section>

      <CleanerSettings />

      {reviewing && <ReviewCleanModal items={reviewing} onConfirm={runClean} onClose={closeReview} />}
    </Stack>
  );
}
