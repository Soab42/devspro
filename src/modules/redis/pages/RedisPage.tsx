import { useCallback, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Badge,
  Button,
  Callout,
  Page,
  Section,
  Stack,
  StatusPill,
  Tabs,
  useResource,
} from "../../../ui";
import { Boxes, DatabaseZap, RefreshCw, Trash2, Workflow } from "../../../ui/icons";
import { flushDb, ping, type RedisConn } from "../api";
import ConnectionBar from "../components/ConnectionBar";
import KeysTab from "../components/KeysTab";
import QueuesTab from "../components/QueuesTab";
import { describeConn, formatDuration, parseStoredConn, serialiseConn, STORAGE_KEY } from "../utils";

type Tab = "keys" | "queues";

const PING_MS = 10000;

function readStored(): RedisConn {
  try {
    return parseStoredConn(localStorage.getItem(STORAGE_KEY));
  } catch {
    return parseStoredConn(null);
  }
}

export default function RedisPage() {
  const module = useModule("redis");
  const confirmAction = useConfirm();
  const [initial] = useState(readStored);
  const [conn, setConn] = useState<RedisConn | null>(null);
  const [tab, setTab] = useState<Tab>("keys");
  const [notice, setNotice] = useState<{ tone: "success" | "danger"; text: string } | null>(null);
  const [flushing, setFlushing] = useState(false);
  // Bumped after a flush so the Keys tab remounts with a fresh scan.
  const [epoch, setEpoch] = useState(0);

  const fetchPing = useCallback(() => {
    if (!conn) return Promise.reject(new Error("not connected"));
    return ping(conn);
  }, [conn]);
  const status = useResource(fetchPing, { enabled: conn !== null, pollMs: PING_MS });

  function connect(next: RedisConn) {
    try {
      localStorage.setItem(STORAGE_KEY, serialiseConn(next));
    } catch {
      // Storage can be unavailable; the connection still works for the session.
    }
    setNotice(null);
    setConn(next);
  }

  async function handleFlush() {
    if (!conn) return;
    const ok = await confirmAction(
      `This deletes every key in ${describeConn(conn)}. Queued jobs, sessions and caches in that database are gone for good.`,
      { title: "Flush this database?", confirmLabel: "Flush DB", danger: true },
    );
    if (!ok) return;
    setFlushing(true);
    try {
      const msg = await flushDb(conn);
      setNotice({ tone: "success", text: msg });
      setEpoch((n) => n + 1);
      void status.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setFlushing(false);
    }
  }

  const info = status.data;
  const connected = conn !== null && info !== null && !status.error;

  return (
    <Page
      title={module.label}
      icon={module.icon}
      description="Browse keys and watch Laravel queues on a local or Docker-published Redis. Nothing here writes to Redis except Delete and Flush DB."
      meta={
        conn && (
          <Stack direction="row" gap={2} align="center">
            {status.error ? (
              <StatusPill variant="bad" label="Unreachable" />
            ) : info ? (
              <StatusPill variant="ok" label={`Redis ${info.version}`} />
            ) : (
              <StatusPill variant="pending" label="Connecting" />
            )}
            {info && !status.error && (
              <>
                <Badge mono>{info.used_memory_human}</Badge>
                <Badge>{info.connected_clients} client{info.connected_clients === 1 ? "" : "s"}</Badge>
                <Badge>{info.total_keys} key{info.total_keys === 1 ? "" : "s"}</Badge>
                <Badge tone="neutral">up {formatDuration(info.uptime_seconds)}</Badge>
              </>
            )}
          </Stack>
        )
      }
      actions={
        conn && (
          <>
            <Button
              variant="secondary"
              icon={RefreshCw}
              loading={status.refreshing}
              onClick={() => {
                void status.refresh();
                setEpoch((n) => n + 1);
              }}
            >
              Refresh
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              disabled={!connected}
              loading={flushing}
              onClick={handleFlush}
            >
              Flush DB
            </Button>
          </>
        )
      }
      tabs={
        connected && (
          <Tabs
            ariaLabel="Redis views"
            active={tab}
            onChange={(id) => setTab(id as Tab)}
            tabs={[
              { id: "keys", label: "Keys", icon: Boxes },
              { id: "queues", label: "Queues", icon: Workflow },
            ]}
          />
        )
      }
    >
      <Stack gap={5}>
        <Section
          title="Connection"
          icon={DatabaseZap}
          collapsible
          defaultCollapsed={false}
          meta={conn && <span className="u-muted">{describeConn(conn)}</span>}
        >
          <ConnectionBar
            key={conn ? "connected" : "idle"}
            initial={conn ?? initial}
            connected={conn !== null}
            busy={conn !== null && status.loading && !status.error}
            onConnect={connect}
            onDisconnect={() => setConn(null)}
          />
        </Section>

        {notice && (
          <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
            {notice.text}
          </Callout>
        )}

        {conn && status.error && (
          <Callout
            tone="danger"
            title="Could not reach Redis"
            actions={
              <Button size="sm" variant="secondary" onClick={() => void status.refresh()}>
                Retry
              </Button>
            }
          >
            {status.error}
          </Callout>
        )}

        {connected && conn && tab === "keys" && <KeysTab key={epoch} conn={conn} />}
        {connected && conn && tab === "queues" && <QueuesTab conn={conn} />}
      </Stack>
    </Page>
  );
}
