import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  CalloutTone,
  Column,
  DataTable,
  Icon,
  IconButton,
  Meter,
  Page,
  PageStatus,
  SearchInput,
  Section,
  SegmentedControl,
  Stack,
  StatusPill,
  StatusVariant,
  Toolbar,
  useResource,
} from "../../../ui";
import { Eraser, Lock, MemoryStick, OctagonX, Pencil, Play, Square } from "../../../ui/icons";
import {
  AppRow,
  AppState,
  CATEGORIES,
  Category,
  dropCaches,
  getWatchdog,
  MemoryInfo,
  memorySnapshot,
  onWatchdog,
  setWatchdog,
  startApp,
  stopApps,
  Support,
  WatchdogConfig,
  WatchdogEvent,
  watchdogLog,
} from "../api";
import EditAppsModal from "../components/EditAppsModal";
import ResetModal from "../components/ResetModal";
import WatchdogSection from "../components/WatchdogSection";
import {
  ADMIN_NOTE,
  canStop,
  formatCpu,
  formatMb,
  FULL_SUPPORT,
  isRunning,
  isWindows,
  processCount,
  STATE_LABEL,
  STATE_RANK,
  watchdogMessage,
  watchdogStatus,
} from "../memory";
import { useDocumentVisible } from "../useDocumentVisible";

const POLL_MS = 4000;

type Filter = "All" | Category;

const STATE_VARIANT: Record<AppState, StatusVariant> = { on: "ok", idle: "warn", off: "neutral" };

const ADMIN_HINT = "Asks for your admin password";

interface Notice {
  tone: CalloutTone;
  text: string;
}

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function MemoryPage() {
  const module = useModule("memory");
  const confirmAction = useConfirm();
  // Polling stops while the window is hidden, and with the page unmounted.
  const visible = useDocumentVisible();
  const snapshot = useResource(memorySnapshot, { pollMs: POLL_MS, enabled: visible });
  const watchdog = useResource(getWatchdog);
  const log = useResource(watchdogLog);

  const [filter, setFilter] = useState<Filter>("All");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [dropping, setDropping] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [closed, setClosed] = useState<WatchdogEvent | null>(null);
  const [dialog, setDialog] = useState<"apps" | "reset" | null>(null);

  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const { refresh: refreshLog } = log;
  const { refresh: refreshSnapshot } = snapshot;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    onWatchdog((event) => {
      setClosed(event);
      void refreshLog();
      if (visibleRef.current) void refreshSnapshot();
    })
      .then((stop) => {
        if (gone) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [refreshLog, refreshSnapshot]);

  const rows = useMemo(() => snapshot.data?.apps ?? [], [snapshot.data]);
  const config = watchdog.data ?? snapshot.data?.watchdog ?? null;
  const support = snapshot.data?.support ?? FULL_SUPPORT;

  const running = (category: string | null) =>
    rows.filter((r) => isRunning(r) && (category === null || r.category === category)).length;
  const countOf = (n: number) => (n > 0 ? n : undefined);
  const filterOptions: { value: Filter; label: string; count?: number }[] = [
    { value: "All", label: "All", count: countOf(running(null)) },
    ...CATEGORIES.map((c) => ({ value: c, label: c, count: countOf(running(c)) })),
  ];

  const q = query.trim().toLowerCase();
  const shown = rows.filter(
    (r) =>
      (filter === "All" || r.category === filter) &&
      (!q || r.name.toLowerCase().includes(q) || r.category.toLowerCase().includes(q)),
  );
  const stoppableInFilter = filter === "All" ? [] : rows.filter((r) => r.category === filter && canStop(r));

  const waitingForAdmin =
    dropping || rows.some((r) => r.needs_admin && busy.has(r.id));

  function markBusy(ids: string[], on: boolean) {
    setBusy((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  }

  async function stop(targets: AppRow[], scope: string | null) {
    const ids = targets.map((r) => r.id);
    const nameOf = (id: string) => targets.find((r) => r.id === id)?.name ?? id;
    markBusy(ids, true);
    setNotice(null);
    try {
      const results = await stopApps(ids);
      const failed = results.filter((r) => !r.ok);
      const stopped = results.length - failed.length;
      if (failed.length === 0) {
        setNotice({
          tone: "success",
          text: scope ? `Stopped ${stopped} ${stopped === 1 ? "app" : "apps"} in ${scope}.` : `Stopped ${nameOf(ids[0])}.`,
        });
      } else {
        setNotice({
          tone: "danger",
          text: [
            stopped > 0 && `Stopped ${stopped} of ${results.length}.`,
            ...failed.map((r) => `Couldn't stop ${nameOf(r.id)}: ${r.message}`),
          ]
            .filter(Boolean)
            .join(" "),
        });
      }
      await snapshot.refresh();
    } catch (e) {
      setNotice({
        tone: "danger",
        text: `Couldn't stop ${scope ? `the apps in ${scope}` : nameOf(ids[0])}: ${messageOf(e)}`,
      });
    } finally {
      markBusy(ids, false);
    }
  }

  async function stopOne(row: AppRow) {
    const ok = await confirmAction(
      <Stack gap={3}>
        <p>
          {row.kind === "service"
            ? `Stops the ${row.name} service`
            : `Closes ${row.name} and its ${processCount(row.process_count)}`}
          {row.ram_mb > 0 && `, freeing about ${formatMb(row.ram_mb)}`}.
        </p>
        {row.kind !== "service" && <p>Unsaved work in it may be lost.</p>}
        {row.needs_admin && <p>{ADMIN_NOTE}</p>}
      </Stack>,
      { title: `Stop ${row.name}?`, confirmLabel: "Stop", danger: true },
    );
    if (ok) await stop([row], null);
  }

  async function stopCategory() {
    if (filter === "All") return;
    const targets = stoppableInFilter;
    const admin = targets.filter((r) => r.needs_admin);
    const total = targets.reduce((sum, r) => sum + r.ram_mb, 0);
    const ok = await confirmAction(
      <Stack gap={3}>
        <p>
          Closes {targets.length === 1 ? "this app" : `these ${targets.length} apps`}, freeing about{" "}
          {formatMb(total)}:
        </p>
        <Stack gap={1}>
          {targets.map((r) => (
            <span key={r.id}>
              {r.name} <span className="u-muted">· {formatMb(r.ram_mb)}</span>
            </span>
          ))}
        </Stack>
        <p>Unsaved work in them may be lost.</p>
        {admin.length > 0 && (
          <p>Stopping {admin.map((r) => r.name).join(", ")} asks for your admin password.</p>
        )}
      </Stack>,
      {
        title: `Stop all in ${filter}?`,
        confirmLabel: targets.length === 1 ? "Stop 1 app" : `Stop ${targets.length} apps`,
        danger: true,
      },
    );
    if (ok) await stop(targets, filter);
  }

  async function start(row: AppRow) {
    markBusy([row.id], true);
    setNotice(null);
    try {
      const result = await startApp(row.id);
      setNotice(
        result.ok
          ? { tone: "success", text: `Started ${row.name}.` }
          : { tone: "danger", text: `Couldn't start ${row.name}: ${result.message}` },
      );
      await snapshot.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: `Couldn't start ${row.name}: ${messageOf(e)}` });
    } finally {
      markBusy([row.id], false);
    }
  }

  async function dropCache(reclaimSwap: boolean) {
    const ok = await confirmAction(
      <Stack gap={3}>
        {isWindows(support) ? (
          <p>
            Frees the RAM Windows keeps as standby cache: recently used files and programs. Nothing
            is closed, though apps may be a little slower while the cache fills up again.
          </p>
        ) : (
          <p>
            Frees the RAM Linux is using to cache files. Nothing is closed, though apps may be a
            little slower while the cache fills up again.
          </p>
        )}
        {reclaimSwap && (
          <p>
            Then moves what&apos;s in swap back into RAM. If there isn&apos;t enough free RAM for
            that, swap is left alone and you&apos;ll see why.
          </p>
        )}
        <p>{ADMIN_NOTE}</p>
      </Stack>,
      {
        title: reclaimSwap ? "Drop cache and reclaim swap?" : "Drop the cache?",
        confirmLabel: reclaimSwap ? "Drop cache and reclaim swap" : "Drop cache",
      },
    );
    if (!ok) return;
    setDropping(true);
    setNotice(null);
    try {
      const [cache, swap] = await dropCaches(reclaimSwap);
      if (!cache?.success) {
        setNotice({ tone: "danger", text: `Couldn't drop the cache: ${cache?.message ?? "no result"}` });
      } else if (reclaimSwap && !swap?.success) {
        setNotice({
          tone: "warning",
          text: `Dropped the cache, but swap was left alone: ${swap?.message ?? "no result"}`,
        });
      } else {
        setNotice({
          tone: "success",
          text: reclaimSwap ? "Dropped the cache and reclaimed swap." : "Dropped the cache.",
        });
      }
      await snapshot.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: `Couldn't drop the cache: ${messageOf(e)}` });
    } finally {
      setDropping(false);
    }
  }

  async function changeWatchdog(next: WatchdogConfig) {
    const previous = config;
    watchdog.setData(next);
    try {
      watchdog.setData(await setWatchdog(next));
    } catch (e) {
      if (previous) watchdog.setData(previous);
      setNotice({ tone: "danger", text: `Couldn't save the watchdog settings: ${messageOf(e)}` });
    }
  }

  const columns: Column<AppRow>[] = [
    {
      key: "name",
      header: "App",
      priority: 1,
      sortValue: (r) => r.name.toLowerCase(),
      render: (r) => (
        <span className="dt-cell-inline">
          <span className="u-truncate">{r.name}</span>
          {r.detected && <Badge tone="info">Detected</Badge>}
          {filter === "All" && <span className="u-muted u-nowrap">{r.category}</span>}
        </span>
      ),
    },
    {
      key: "state",
      header: "Status",
      priority: 1,
      sortValue: (r) => STATE_RANK[r.state],
      render: (r) => <StatusPill variant={STATE_VARIANT[r.state]} label={STATE_LABEL[r.state]} />,
    },
    {
      key: "ram",
      header: "RAM",
      priority: 2,
      align: "end",
      sortValue: (r) => r.ram_mb,
      render: (r) =>
        isRunning(r) ? (
          <span className="u-tnum" title={processCount(r.process_count)}>
            {formatMb(r.ram_mb)}
          </span>
        ) : (
          <span className="u-muted">—</span>
        ),
    },
    {
      key: "cpu",
      header: "CPU",
      priority: 3,
      align: "end",
      sortValue: (r) => r.cpu_pct,
      render: (r) =>
        isRunning(r) ? (
          <span className="u-tnum">{formatCpu(r.cpu_pct)}</span>
        ) : (
          <span className="u-muted">—</span>
        ),
    },
    {
      key: "actions",
      header: "",
      headerLabel: "Actions",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => <RowActions row={r} busy={busy.has(r.id)} onStop={stopOne} onStart={start} />,
    },
  ];

  const status: PageStatus = snapshot.data
    ? { kind: "ready" }
    : snapshot.error
      ? { kind: "error", message: snapshot.error, retry: snapshot.refresh }
      : { kind: "loading" };

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={status}
      description="Which apps are using your RAM, and a watchdog that closes one when memory runs out. DevsPro and your desktop session are never stopped from here."
      actions={
        <>
          <Button variant="danger" icon={OctagonX} onClick={() => setDialog("reset")}>
            Full reset…
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {closed && (
          <Callout tone={closed.ok ? "warning" : "danger"} onDismiss={() => setClosed(null)}>
            {watchdogMessage(closed)}
          </Callout>
        )}
        {waitingForAdmin && <Callout tone="info">Waiting for your admin password…</Callout>}
        {notice && (
          <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
            {notice.text}
          </Callout>
        )}
        {snapshot.data && snapshot.error && (
          <Callout tone="danger">Couldn&apos;t refresh: {snapshot.error}</Callout>
        )}

        {snapshot.data && (
          <Summary
            memory={snapshot.data.memory}
            support={support}
            config={config}
            dropping={dropping}
            onDropCache={(reclaimSwap) => void dropCache(reclaimSwap)}
          />
        )}

        <Section
          title="Apps"
          padding="none"
          actions={
            <Button variant="secondary" icon={Pencil} onClick={() => setDialog("apps")}>
              Edit apps…
            </Button>
          }
          toolbar={
            <Toolbar
              end={
                filter !== "All" && (
                  <Button
                    variant="danger"
                    icon={Square}
                    disabled={stoppableInFilter.length === 0}
                    onClick={() => void stopCategory()}
                  >
                    Stop all in {filter}
                  </Button>
                )
              }
            >
              <SearchInput value={query} onChange={setQuery} placeholder="Filter by app name" />
              <SegmentedControl
                ariaLabel="Filter by category"
                value={filter}
                onChange={setFilter}
                options={filterOptions}
              />
            </Toolbar>
          }
        >
          <DataTable
            columns={columns}
            rows={shown}
            rowKey={(r) => r.id}
            loading={snapshot.loading}
            defaultSort={{ key: "ram", dir: "desc" }}
            caption="Apps and the memory they use"
            empty={{
              icon: MemoryStick,
              title: "No matching apps",
              description: "Nothing matches the current search and category.",
            }}
          />
        </Section>

        <WatchdogSection config={config} onChange={(next) => void changeWatchdog(next)} events={log.data} />
      </Stack>

      {dialog === "apps" && (
        <EditAppsModal
          support={support}
          onClose={() => setDialog(null)}
          onSaved={(count) => {
            setDialog(null);
            setNotice({ tone: "success", text: `Saved ${count === 1 ? "1 app" : `${count} apps`}.` });
            void snapshot.refresh();
          }}
        />
      )}
      {dialog === "reset" && (
        <ResetModal
          support={support}
          onClose={() => setDialog(null)}
          onFinished={() => void snapshot.refresh()}
        />
      )}
    </Page>
  );
}

function Summary({
  memory,
  support,
  config,
  dropping,
  onDropCache,
}: {
  memory: MemoryInfo;
  support: Support;
  config: WatchdogConfig | null;
  dropping: boolean;
  onDropCache: (reclaimSwap: boolean) => void;
}) {
  // Windows' swap is its page file, and the figure is an estimate (see the backend).
  const swapLabel = isWindows(support) ? "Page file" : "Swap";
  return (
    <Section
      title="RAM"
      meta={
        config && (
          <StatusPill variant={config.enabled ? "ok" : "neutral"} label={watchdogStatus(config)} />
        )
      }
      // Beside the figures they change, and labelled: as an icon-only menu
      // among the page actions they were easy to miss.
      actions={
        <>
          <Button size="sm" variant="secondary" icon={Eraser} disabled={dropping} onClick={() => onDropCache(false)}>
            Drop cache
          </Button>
          {/* Windows has no way to pull paged-out memory back in on demand. */}
          {support.reclaim_swap && (
            <Button
              size="sm"
              variant="secondary"
              disabled={dropping || memory.swap_used_mb === 0}
              title={memory.swap_used_mb === 0 ? "Nothing is in swap" : undefined}
              onClick={() => onDropCache(true)}
            >
              Reclaim swap
            </Button>
          )}
        </>
      }
    >
      <Stack gap={3}>
        <Meter label="Used" percent={memory.used_pct} valueLabel={`${memory.used_pct}%`} />
        <Stack direction="row" gap={5} wrap>
          <span>
            <strong className="u-tnum">{formatMb(memory.available_mb)}</strong> free of{" "}
            {formatMb(memory.total_mb)}
          </span>
          <span className="u-secondary u-tnum">
            {memory.swap_total_mb > 0
              ? `${swapLabel}: ${formatMb(memory.swap_used_mb)} used of ${formatMb(memory.swap_total_mb)}`
              : `No ${swapLabel.toLowerCase()}`}
          </span>
        </Stack>
      </Stack>
    </Section>
  );
}

function RowActions({
  row,
  busy,
  onStop,
  onStart,
}: {
  row: AppRow;
  busy: boolean;
  onStop: (row: AppRow) => void;
  onStart: (row: AppRow) => void;
}) {
  const suffix = row.needs_admin ? ` (${ADMIN_HINT.toLowerCase()})` : "";
  let action: ReactNode;
  if (isRunning(row)) {
    action = (
      <IconButton
        icon={Square}
        label={
          row.protected
            ? `${row.name} can't be stopped here: it keeps DevsPro or your desktop running`
            : `Stop ${row.name}${suffix}`
        }
        disabled={row.protected}
        loading={busy}
        onClick={() => onStop(row)}
      />
    );
  } else if (row.can_start) {
    action = (
      <IconButton
        icon={Play}
        label={`Start ${row.name}${suffix}`}
        loading={busy}
        onClick={() => onStart(row)}
      />
    );
  } else {
    action = <IconButton icon={Play} label={`${row.name} has no start command`} disabled />;
  }
  return (
    <ActionGroup>
      {row.needs_admin && (
        <span className="u-muted" title={ADMIN_HINT}>
          <Icon icon={Lock} size="xs" aria-label={ADMIN_HINT} />
        </span>
      )}
      {action}
    </ActionGroup>
  );
}
